mod app_menu;
/// Pure-logic core for the XDG GlobalShortcuts desktop portal used by Wayland global hotkeys.
pub(crate) mod global_hotkey_portal;
mod keyboard;
mod keystroke;
/// Cross-platform single instance enforcement using Unix domain sockets and Windows named mutexes.
pub mod single_instance;
/// Cross-platform window tab manager for Windows and Linux backends.
pub mod tab_manager;
/// Pure Rust utility for computing window bounds from a semantic [`WindowPosition`].
pub mod window_positioner;

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "windows"))]
mod webview_common;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;

#[cfg(target_os = "macos")]
mod mac;

#[cfg(any(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        any(feature = "x11", feature = "wayland")
    ),
    all(target_os = "macos", feature = "macos-blade")
))]
mod blade;

#[cfg(any(test, feature = "test-support"))]
mod test;

#[cfg(test)]
mod tests;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(all(
    feature = "screen-capture",
    any(
        target_os = "windows",
        all(
            any(target_os = "linux", target_os = "freebsd"),
            any(feature = "wayland", feature = "x11"),
        )
    )
))]
pub(crate) mod scap_screen_capture;

use crate::{
    Action, AnyWindowHandle, App, AsyncWindowContext, BackgroundExecutor, Bounds,
    DEFAULT_WINDOW_SIZE, DevicePixels, DispatchEventResult, Font, FontFeature, FontId, FontMetrics,
    FontRun, ForegroundExecutor, GlyphId, GpuSpecs, ImageSource, Keymap, LineLayout, Pixels,
    PlatformInput, Point, RenderGlyphParams, RenderImage, RenderImageParams, RenderSvgParams,
    Scene, ShapedGlyph, ShapedRun, SharedString, Size, SvgRenderer, SvgSize, SystemWindowTab, Task,
    TaskLabel, Window, WindowControlArea, WindowPlacement, hash, point,
    print::PlatformPrintJob,
    px, size,
    webview::{PlatformWebView, PlatformWebViewCommand},
};
use anyhow::Result;
use async_task::Runnable;
use futures::channel::oneshot;
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder as _, Frame};
use parking::Unparker;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use schemars::JsonSchema;
use seahash::SeaHasher;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::ops;
use std::time::{Duration, Instant};
use std::{
    fmt::{self, Debug},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
use strum::EnumIter;
use uuid::Uuid;

pub use app_menu::*;
pub use keyboard::*;
pub use keystroke::*;
pub use single_instance::*;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
pub(crate) use mac::*;
pub use semantic_version::SemanticVersion;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use test::*;
#[cfg(target_os = "windows")]
pub(crate) use windows::*;

#[cfg(any(test, feature = "test-support"))]
pub use test::{TestDispatcher, TestScreenCaptureSource, TestScreenCaptureStream};

/// Emits a single `log::warn!` for an API that is accepted but not implemented on the
/// current platform, so callers learn the call is a no-op without flooding the log on
/// every invocation. Each expansion site warns at most once for the process lifetime.
macro_rules! warn_unsupported_once {
    ($api:literal) => {{
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            log::warn!(concat!(
                $api,
                " is not supported on this platform; ignoring the call"
            ));
        });
    }};
}

/// Returns a background executor for the current platform.
pub fn background_executor() -> BackgroundExecutor {
    current_platform(true).background_executor()
}

/// Attempts to create a background executor without panicking if platform initialization fails.
pub fn try_background_executor() -> anyhow::Result<BackgroundExecutor> {
    Ok(try_current_platform(true)?.background_executor())
}

#[cfg(target_os = "macos")]
pub(crate) fn current_platform(headless: bool) -> Rc<dyn Platform> {
    try_current_platform(headless).expect("failed to initialize the macOS platform")
}

#[cfg(target_os = "macos")]
pub(crate) fn try_current_platform(headless: bool) -> anyhow::Result<Rc<dyn Platform>> {
    Ok(Rc::new(MacPlatform::new(headless)))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) fn current_platform(headless: bool) -> Rc<dyn Platform> {
    match try_current_platform(headless) {
        Ok(platform) => platform,
        Err(platform_error) if !headless => {
            log::error!(
                "failed to initialize the graphical platform; using headless backend: {platform_error:#}"
            );
            Rc::new(HeadlessClient::new().unwrap_or_else(|headless_error| {
                panic!(
                    "graphical platform initialization failed ({platform_error:#}); headless fallback also failed ({headless_error:#})"
                )
            }))
        }
        Err(error) => panic!("failed to initialize the headless platform: {error:#}"),
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) fn try_current_platform(headless: bool) -> anyhow::Result<Rc<dyn Platform>> {
    if headless {
        return Ok(Rc::new(HeadlessClient::new()?));
    }

    match guess_compositor() {
        #[cfg(feature = "wayland")]
        "Wayland" => Ok(Rc::new(WaylandClient::new().map_err(|error| {
            anyhow::anyhow!("failed to initialize Wayland: {error:#}")
        })?)),

        #[cfg(feature = "x11")]
        "X11" => Ok(Rc::new(X11Client::new().map_err(|error| {
            anyhow::anyhow!("failed to initialize X11: {error:#}")
        })?)),

        "Headless" => Ok(Rc::new(HeadlessClient::new()?)),
        compositor => anyhow::bail!("unsupported compositor selection {compositor:?}"),
    }
}

/// Return which compositor we're guessing we'll use.
/// Does not attempt to connect to the given compositor
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[inline]
pub fn guess_compositor() -> &'static str {
    if std::env::var_os("KAEL_HEADLESS").is_some() {
        return "Headless";
    }

    #[cfg(feature = "wayland")]
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY");
    #[cfg(not(feature = "wayland"))]
    let wayland_display: Option<std::ffi::OsString> = None;

    #[cfg(feature = "x11")]
    let x11_display = std::env::var_os("DISPLAY");
    #[cfg(not(feature = "x11"))]
    let x11_display: Option<std::ffi::OsString> = None;

    let use_wayland = wayland_display.is_some_and(|display| !display.is_empty());
    let use_x11 = x11_display.is_some_and(|display| !display.is_empty());

    if use_wayland {
        "Wayland"
    } else if use_x11 {
        "X11"
    } else {
        "Headless"
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn current_platform(_headless: bool) -> Rc<dyn Platform> {
    try_current_platform(false)
        .inspect_err(|err| show_error("Failed to launch", err.to_string()))
        .expect("failed to initialize the Windows platform")
}

#[cfg(target_os = "windows")]
pub(crate) fn try_current_platform(_headless: bool) -> anyhow::Result<Rc<dyn Platform>> {
    Ok(Rc::new(WindowsPlatform::new()?))
}

pub(crate) trait Platform: 'static {
    fn background_executor(&self) -> BackgroundExecutor;
    fn foreground_executor(&self) -> ForegroundExecutor;
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>);
    fn quit(&self);
    fn restart(&self, binary_path: Option<PathBuf>);
    fn activate(&self, ignoring_other_apps: bool);
    fn hide(&self);
    fn hide_other_apps(&self);
    fn unhide_other_apps(&self);

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>>;
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>>;
    fn active_window(&self) -> Option<AnyWindowHandle>;
    fn cursor_position(&self) -> Option<Point<Pixels>> {
        None
    }
    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        None
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool;
    #[cfg(not(feature = "screen-capture"))]
    fn is_screen_capture_supported(&self) -> bool {
        false
    }
    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(&self)
    -> oneshot::Receiver<Result<Vec<Rc<dyn ScreenCaptureSource>>>>;
    #[cfg(not(feature = "screen-capture"))]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<anyhow::Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
        let (sources_tx, sources_rx) = oneshot::channel();
        sources_tx
            .send(Err(anyhow::anyhow!(
                "gpui was compiled without the screen-capture feature"
            )))
            .ok();
        sources_rx
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>>;

    /// Returns the appearance of the application's windows.
    fn window_appearance(&self) -> WindowAppearance;

    fn open_url(&self, url: &str);
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>);
    fn register_url_scheme(&self, url: &str) -> Task<Result<()>>;

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>>;
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>>;
    fn can_select_mixed_files_and_dirs(&self) -> bool;
    fn reveal_path(&self, path: &Path);
    fn open_with_system(&self, path: &Path);
    fn move_path_to_trash(&self, path: &Path) -> Result<()> {
        Err(anyhow::anyhow!(
            "platform trash/recycle is not supported for {}",
            path.display()
        ))
    }

    fn on_quit(&self, callback: Box<dyn FnMut()>);
    fn on_reopen(&self, callback: Box<dyn FnMut()>);

    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap);
    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        None
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap);
    fn dock_menu_action_count(&self) -> Option<usize> {
        None
    }
    fn perform_dock_menu_action(&self, _action: usize) {}
    fn add_recent_document(&self, _path: &Path) {}
    fn clear_recent_documents(&self) {}
    fn update_jump_list(
        &self,
        _menus: Vec<MenuItem>,
        _entries: Vec<SmallVec<[PathBuf; 2]>>,
    ) -> Vec<SmallVec<[PathBuf; 2]>> {
        Vec::new()
    }
    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>);
    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>);
    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>);

    fn compositor_name(&self) -> &'static str {
        ""
    }
    fn app_path(&self) -> Result<PathBuf>;
    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf>;

    fn set_cursor_style(&self, style: CursorStyle);
    fn should_auto_hide_scrollbars(&self) -> bool;

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_to_primary(&self, item: ClipboardItem);
    fn write_to_clipboard(&self, item: ClipboardItem);
    fn clear_clipboard(&self) {}
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem>;
    fn read_from_clipboard(&self) -> Option<ClipboardItem>;

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>>;
    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>>;
    fn delete_credentials(&self, url: &str) -> Task<Result<()>>;

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout>;
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper>;
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>);

    fn set_tray_icon(&self, _icon: Option<&[u8]>) {}
    fn set_tray_menu(&self, _menu: Vec<TrayMenuItem>) {}
    fn set_tray_tooltip(&self, _tooltip: &str) {}
    fn set_tray_panel_mode(&self, _enabled: bool) {}
    fn get_tray_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        None
    }
    fn on_tray_icon_event(&self, _callback: Box<dyn FnMut(TrayIconEvent)>) {}
    fn on_tray_menu_action(&self, _callback: Box<dyn FnMut(SharedString)>) {}

    fn register_global_hotkey(&self, _id: u32, _keystroke: &Keystroke) -> Result<()> {
        Err(anyhow::anyhow!(
            "Global hotkeys not supported on this platform"
        ))
    }
    fn unregister_global_hotkey(&self, _id: u32) {}
    fn on_global_hotkey(&self, _callback: Box<dyn FnMut(u32)>) {}
    fn on_global_hotkey_up(&self, _callback: Box<dyn FnMut(u32)>) {}

    fn focused_window_info(&self) -> Option<FocusedWindowInfo> {
        None
    }

    fn accessibility_status(&self) -> PermissionStatus {
        PermissionStatus::Granted
    }
    fn request_accessibility_permission(&self) {}

    fn microphone_status(&self) -> PermissionStatus {
        PermissionStatus::Granted
    }
    fn request_microphone_permission(&self, callback: Box<dyn FnOnce(bool)>) {
        callback(true);
    }
    fn camera_status(&self) -> PermissionStatus {
        PermissionStatus::Granted
    }
    fn request_camera_permission(&self, callback: Box<dyn FnOnce(bool)>) {
        callback(true);
    }

    fn set_auto_launch(&self, _app_id: &str, _enabled: bool) -> Result<()> {
        Err(anyhow::anyhow!(
            "Auto-launch not supported on this platform"
        ))
    }
    fn is_auto_launch_enabled(&self, _app_id: &str) -> bool {
        false
    }

    fn show_notification(&self, _title: &str, _body: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "Notifications not supported on this platform"
        ))
    }

    fn show_notification_with_actions(
        &self,
        _title: &str,
        _body: &str,
        _actions: &[NotificationAction],
        _callback: Box<dyn FnMut(String)>,
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "Notifications with actions not supported on this platform"
        ))
    }

    fn set_keep_alive_without_windows(&self, _keep_alive: bool) {}

    fn on_system_power_event(&self, _callback: Box<dyn FnMut(SystemPowerEvent)>) {}
    fn start_power_save_blocker(&self, _kind: PowerSaveBlockerKind) -> Option<u32> {
        None
    }
    fn stop_power_save_blocker(&self, _id: u32) {}
    fn power_mode(&self) -> PowerMode {
        PowerMode::Performance
    }
    fn system_power_source(&self) -> SystemPowerSource {
        SystemPowerSource::Unknown
    }
    fn battery_percentage(&self) -> Option<u8> {
        None
    }
    /// Whether the OS "reduce motion" accessibility preference is enabled.
    fn should_reduce_motion(&self) -> bool {
        false
    }
    fn system_idle_time(&self) -> Option<Duration> {
        None
    }
    fn network_status(&self) -> NetworkStatus {
        NetworkStatus::Online
    }
    fn on_network_status_change(&self, _callback: Box<dyn FnMut(NetworkStatus)>) {}
    fn on_media_key_event(&self, _callback: Box<dyn FnMut(MediaKeyEvent)>) {}
    fn request_user_attention(&self, _attention_type: AttentionType) {}
    fn cancel_user_attention(&self) {}
    fn set_dock_badge(&self, _label: Option<&str>) {}
    fn show_context_menu(
        &self,
        _position: Point<Pixels>,
        _items: Vec<TrayMenuItem>,
        _callback: Box<dyn FnMut(SharedString)>,
    ) {
    }
    fn show_dialog(&self, _options: DialogOptions) -> oneshot::Receiver<usize> {
        let (tx, rx) = oneshot::channel();
        tx.send(0).ok();
        rx
    }
    fn os_info(&self) -> OsInfo {
        OsInfo {
            name: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            version: String::new().into(),
            locale: String::new().into(),
            hostname: String::new().into(),
        }
    }
    fn biometric_status(&self) -> BiometricStatus {
        BiometricStatus::Unavailable
    }
    fn authenticate_biometric(&self, _reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
        callback(false);
    }
}

/// A handle to a platform's display, e.g. a monitor or laptop screen.
pub trait PlatformDisplay: Send + Sync + Debug {
    /// Get the ID for this display
    fn id(&self) -> DisplayId;

    /// Returns a stable identifier for this display that can be persisted and used
    /// across system restarts.
    fn uuid(&self) -> Result<Uuid>;

    /// Get the bounds for this display
    fn bounds(&self) -> Bounds<Pixels>;

    /// Get the default bounds for this display to place a window
    fn default_bounds(&self) -> Bounds<Pixels> {
        let center = self.bounds().center();
        let offset = DEFAULT_WINDOW_SIZE / 2.0;
        let origin = point(center.x - offset.width, center.y - offset.height);
        Bounds::new(origin, DEFAULT_WINDOW_SIZE)
    }

    /// The refresh rate of this display in hertz (e.g. `60.0`, `120.0`).
    ///
    /// Returns `None` when the platform cannot report a rate for the display
    /// (for example a virtual or headless display, or a panel that does not
    /// advertise a fixed rate). Callers should fall back to a sensible default.
    fn refresh_rate(&self) -> Option<f32> {
        None
    }

    /// Logical-to-device pixel scale factor for this display.
    ///
    /// Returns `1.0` when the platform cannot report a display-specific scale.
    fn scale_factor(&self) -> f32 {
        1.0
    }
}

/// Metadata for a given [ScreenCaptureSource]
#[derive(Clone)]
pub struct SourceMetadata {
    /// Opaque identifier of this screen.
    pub id: u64,
    /// Human-readable label for this source.
    pub label: Option<SharedString>,
    /// Whether this source is the main display.
    pub is_main: Option<bool>,
    /// Video resolution of this source.
    pub resolution: Size<DevicePixels>,
}

/// A source of on-screen video content that can be captured.
pub trait ScreenCaptureSource {
    /// Returns metadata for this source.
    fn metadata(&self) -> Result<SourceMetadata>;

    /// Start capture video from this source, invoking the given callback
    /// with each frame.
    fn stream(
        &self,
        foreground_executor: &ForegroundExecutor,
        frame_callback: Box<dyn Fn(ScreenCaptureFrame) + Send>,
    ) -> oneshot::Receiver<Result<Box<dyn ScreenCaptureStream>>>;
}

/// A video stream captured from a screen.
pub trait ScreenCaptureStream {
    /// Returns metadata for this source.
    fn metadata(&self) -> Result<SourceMetadata>;
}

/// A frame of video captured from a screen.
pub struct ScreenCaptureFrame(pub PlatformScreenCaptureFrame);

/// An opaque identifier for a hardware display
#[derive(PartialEq, Eq, Hash, Copy, Clone, Serialize, Deserialize)]
pub struct DisplayId(pub(crate) u32);

impl From<DisplayId> for u32 {
    fn from(id: DisplayId) -> Self {
        id.0
    }
}

impl Debug for DisplayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DisplayId({})", self.0)
    }
}

unsafe impl Send for DisplayId {}

/// Which part of the window to resize
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    /// The top edge
    Top,
    /// The top right corner
    TopRight,
    /// The right edge
    Right,
    /// The bottom right corner
    BottomRight,
    /// The bottom edge
    Bottom,
    /// The bottom left corner
    BottomLeft,
    /// The left edge
    Left,
    /// The top left corner
    TopLeft,
}

impl ResizeEdge {
    /// Stable label for diagnostics, generated resize handles, and custom chrome.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::TopRight => "top-right",
            Self::Right => "right",
            Self::BottomRight => "bottom-right",
            Self::Bottom => "bottom",
            Self::BottomLeft => "bottom-left",
            Self::Left => "left",
            Self::TopLeft => "top-left",
        }
    }
}

/// A type to describe the appearance of a window
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default)]
pub enum WindowDecorations {
    #[default]
    /// Server side decorations
    Server,
    /// Client side decorations
    Client,
}

impl WindowDecorations {
    /// Stable label for diagnostics and generated UI.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
        }
    }
}

/// A type to describe how this window is currently configured
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default)]
pub enum Decorations {
    /// The window is configured to use server side decorations
    #[default]
    Server,
    /// The window is configured to use client side decorations
    Client {
        /// The edge tiling state
        tiling: Tiling,
    },
}

/// What window controls this platform supports
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct WindowControls {
    /// Whether this platform supports fullscreen
    pub fullscreen: bool,
    /// Whether this platform supports maximize
    pub maximize: bool,
    /// Whether this platform supports minimize
    pub minimize: bool,
    /// Whether this platform supports a window menu
    pub window_menu: bool,
}

impl Default for WindowControls {
    fn default() -> Self {
        // Assume that we can do anything, unless told otherwise
        Self {
            fullscreen: true,
            maximize: true,
            minimize: true,
            window_menu: true,
        }
    }
}

impl WindowControls {
    /// Number of supported native window-control affordances.
    pub fn supported_count(&self) -> usize {
        [
            self.fullscreen,
            self.maximize,
            self.minimize,
            self.window_menu,
        ]
        .into_iter()
        .filter(|supported| *supported)
        .count()
    }

    /// Whether all standard native window controls are supported.
    pub fn supports_all(&self) -> bool {
        self.supported_count() == 4
    }

    /// Whether no standard native window controls are supported.
    pub fn supports_none(&self) -> bool {
        self.supported_count() == 0
    }

    /// Whether this platform exposes a native maximize or fullscreen affordance.
    pub fn has_zoom_control(&self) -> bool {
        self.maximize || self.fullscreen
    }

    /// Content-safe summary for generated custom titlebars.
    pub fn to_text(&self) -> String {
        format!(
            "window controls: supported {}, fullscreen {}, maximize {}, minimize {}, window-menu {}, zoom {}",
            self.supported_count(),
            bool_text(self.fullscreen),
            bool_text(self.maximize),
            bool_text(self.minimize),
            bool_text(self.window_menu),
            bool_text(self.has_zoom_control())
        )
    }
}

/// A type to describe which sides of the window are currently tiled in some way
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default)]
pub struct Tiling {
    /// Whether the top edge is tiled
    pub top: bool,
    /// Whether the left edge is tiled
    pub left: bool,
    /// Whether the right edge is tiled
    pub right: bool,
    /// Whether the bottom edge is tiled
    pub bottom: bool,
}

impl Tiling {
    /// Initializes a [`Tiling`] type with all sides tiled
    pub fn tiled() -> Self {
        Self {
            top: true,
            left: true,
            right: true,
            bottom: true,
        }
    }

    /// Whether any edge is tiled
    pub fn is_tiled(&self) -> bool {
        self.top || self.left || self.right || self.bottom
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub(crate) struct RequestFrameOptions {
    pub(crate) require_presentation: bool,
    /// Force refresh of all rendering states when true
    pub(crate) force_render: bool,
}

pub(crate) trait PlatformWindow: HasWindowHandle + HasDisplayHandle {
    fn bounds(&self) -> Bounds<Pixels>;
    fn is_maximized(&self) -> bool;
    fn window_bounds(&self) -> WindowBounds;
    fn content_size(&self) -> Size<Pixels>;
    fn resize(&mut self, size: Size<Pixels>);
    fn scale_factor(&self) -> f32;
    fn appearance(&self) -> WindowAppearance;
    fn display(&self) -> Option<Rc<dyn PlatformDisplay>>;
    fn mouse_position(&self) -> Point<Pixels>;
    fn modifiers(&self) -> Modifiers;
    fn capslock(&self) -> Capslock;
    fn set_input_handler(&mut self, input_handler: PlatformInputHandler);
    fn take_input_handler(&mut self) -> Option<PlatformInputHandler>;
    fn prompt(
        &self,
        level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>>;
    fn activate(&self);
    fn is_active(&self) -> bool;
    fn is_hovered(&self) -> bool;
    fn set_title(&mut self, title: &str);
    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance);
    fn set_opacity(&self, _opacity: f32) {
        warn_unsupported_once!("set_opacity");
    }
    fn set_always_on_top(&self, _always_on_top: bool) {
        warn_unsupported_once!("set_always_on_top");
    }
    fn set_frame_polling(&self, _active: bool) {}
    fn close(&self) {
        warn_unsupported_once!("close");
    }
    fn minimize(&self);
    fn zoom(&self);
    fn toggle_fullscreen(&self);
    fn is_fullscreen(&self) -> bool;
    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>);
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>);
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>);
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>);
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>);
    fn on_moved(&self, callback: Box<dyn FnMut()>);
    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>);
    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>);
    fn on_close(&self, callback: Box<dyn FnOnce()>);
    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>);
    fn sync_webviews(&mut self, _webviews: &[PlatformWebView]) {}
    fn dispatch_webview_command(&mut self, _command: PlatformWebViewCommand) -> anyhow::Result<()> {
        Ok(())
    }
    fn print(&mut self, _job: PlatformPrintJob) -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "printing is not supported on this platform"
        ))
    }
    fn show_print_dialog(&mut self, _job: PlatformPrintJob) -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "printing is not supported on this platform"
        ))
    }
    fn draw(&self, scene: &Scene);
    fn completed_frame(&self) {}
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas>;

    // macOS specific methods
    fn get_title(&self) -> String {
        String::new()
    }
    fn tabbed_windows(&self) -> Option<Vec<SystemWindowTab>> {
        None
    }
    fn tab_bar_visible(&self) -> bool {
        false
    }
    /// Marks the window as having unsaved changes (the macOS document-edited dot).
    /// macOS-only; a no-op on other platforms.
    fn set_edited(&mut self, _edited: bool) {
        warn_unsupported_once!("set_edited");
    }
    /// Shows the system emoji & symbols palette. macOS-only; a no-op elsewhere.
    fn show_character_palette(&self) {
        warn_unsupported_once!("show_character_palette");
    }
    /// Performs the configured titlebar double-click action (zoom/minimize).
    /// macOS-only; a no-op elsewhere.
    fn titlebar_double_click(&self) {
        warn_unsupported_once!("titlebar_double_click");
    }
    /// Registers a handler for the native "move tab to new window" command.
    /// macOS window tabbing only; the callback is never invoked on other platforms.
    fn on_move_tab_to_new_window(&self, _callback: Box<dyn FnMut()>) {
        warn_unsupported_once!("on_move_tab_to_new_window (macOS window tabbing)");
    }
    /// Registers a handler for the native "merge all windows" command.
    /// macOS window tabbing only; the callback is never invoked on other platforms.
    fn on_merge_all_windows(&self, _callback: Box<dyn FnMut()>) {
        warn_unsupported_once!("on_merge_all_windows (macOS window tabbing)");
    }
    /// Registers a handler for the native "select previous tab" command.
    /// macOS window tabbing only; the callback is never invoked on other platforms.
    fn on_select_previous_tab(&self, _callback: Box<dyn FnMut()>) {
        warn_unsupported_once!("on_select_previous_tab (macOS window tabbing)");
    }
    /// Registers a handler for the native "select next tab" command.
    /// macOS window tabbing only; the callback is never invoked on other platforms.
    fn on_select_next_tab(&self, _callback: Box<dyn FnMut()>) {
        warn_unsupported_once!("on_select_next_tab (macOS window tabbing)");
    }
    /// Registers a handler for the native "toggle tab bar" command.
    /// macOS window tabbing only; the callback is never invoked on other platforms.
    fn on_toggle_tab_bar(&self, _callback: Box<dyn FnMut()>) {
        warn_unsupported_once!("on_toggle_tab_bar (macOS window tabbing)");
    }
    /// Merges all open windows into one tabbed window. macOS-only; a no-op elsewhere.
    fn merge_all_windows(&self) {
        warn_unsupported_once!("merge_all_windows (macOS window tabbing)");
    }
    /// Moves the current tab into its own window. macOS-only; a no-op elsewhere.
    fn move_tab_to_new_window(&self) {
        warn_unsupported_once!("move_tab_to_new_window (macOS window tabbing)");
    }
    /// Toggles the macOS tab overview (Exposé-style). macOS-only; a no-op elsewhere.
    fn toggle_window_tab_overview(&self) {
        warn_unsupported_once!("toggle_window_tab_overview (macOS window tabbing)");
    }
    /// Sets the tabbing identifier used to group windows into native tabs.
    /// macOS-only; a no-op elsewhere.
    fn set_tabbing_identifier(&self, _identifier: Option<String>) {
        warn_unsupported_once!("set_tabbing_identifier (macOS window tabbing)");
    }

    #[cfg(target_os = "windows")]
    fn get_raw_handle(&self) -> windows::HWND;

    // Linux specific methods
    fn inner_window_bounds(&self) -> WindowBounds {
        self.window_bounds()
    }
    fn request_decorations(&self, _decorations: WindowDecorations) {}
    fn show_window_menu(&self, _position: Point<Pixels>) {}
    fn start_window_move(&self) {}
    fn start_window_resize(&self, _edge: ResizeEdge) {}
    fn window_decorations(&self) -> Decorations {
        Decorations::Server
    }
    fn set_app_id(&mut self, _app_id: &str) {}
    fn map_window(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn window_controls(&self) -> WindowControls {
        WindowControls::default()
    }
    fn set_client_inset(&self, _inset: Pixels) {}
    fn gpu_specs(&self) -> Option<GpuSpecs>;

    fn update_ime_position(&self, _bounds: Bounds<Pixels>);

    fn show(&self) {}
    fn hide(&self) {}
    fn is_visible(&self) -> bool {
        true
    }
    fn set_mouse_passthrough(&self, _passthrough: bool) {}
    /// Set a soft byte budget for this window's glyph/sprite atlas. When set, least-recently
    /// -used atlas tiles are evicted to the budget at the end of each frame. `None` (default)
    /// disables eviction. No-op on backends that do not yet implement atlas eviction.
    fn set_atlas_byte_budget(&self, _budget: Option<u64>) {}
    fn set_progress_bar(&self, _state: ProgressBarState) {}

    /// Get the display refresh rate for this window's current display.
    /// Returns `None` if the refresh rate cannot be determined.
    #[allow(dead_code)]
    fn display_refresh_rate(&self) -> Option<f32> {
        None
    }

    /// Update the platform accessibility backend with the current accessibility tree.
    ///
    /// Returns any normalized assistive-technology action requests that were
    /// delivered by the platform adapter while synchronizing the tree.
    fn update_accessibility_tree(
        &mut self,
        _tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        Vec::new()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn as_test(&mut self) -> Option<&mut TestWindow> {
        None
    }
}

/// This type is public so that our test macro can generate and use it, but it should not
/// be considered part of our public API.
#[doc(hidden)]
pub trait PlatformDispatcher: Send + Sync {
    fn is_main_thread(&self) -> bool;
    fn dispatch(&self, runnable: Runnable, label: Option<TaskLabel>);
    fn dispatch_on_main_thread(&self, runnable: Runnable);
    fn dispatch_after(&self, duration: Duration, runnable: Runnable);
    fn park(&self, timeout: Option<Duration>) -> bool;
    fn unparker(&self) -> Unparker;
    fn now(&self) -> Instant {
        Instant::now()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn as_test(&self) -> Option<&TestDispatcher> {
        None
    }
}

pub(crate) trait PlatformTextSystem: Send + Sync {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()>;
    fn all_font_names(&self) -> Vec<String>;
    fn font_id(&self, descriptor: &Font) -> Result<FontId>;
    fn font_metrics(&self, font_id: FontId) -> FontMetrics;
    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>>;
    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>>;
    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId>;
    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>>;
    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)>;

    /// Whether this backend can rasterize glyphs with per-channel (LCD RGB)
    /// subpixel coverage. Backends that only produce grayscale alpha masks
    /// return `false`, in which case the renderer falls back to grayscale
    /// antialiasing for text.
    fn supports_subpixel_glyphs(&self) -> bool {
        false
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout;

    /// Layout text with additional OpenType font features applied.
    ///
    /// The default implementation ignores the features and delegates to `layout_line`.
    /// Platform backends that support OpenType features should override this method.
    fn layout_line_with_features(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        features: &[FontFeature],
    ) -> LineLayout {
        let _ = features;
        self.layout_line(text, font_size, runs)
    }
}

pub(crate) struct NoopTextSystem;

impl NoopTextSystem {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl PlatformTextSystem for NoopTextSystem {
    fn add_fonts(&self, _fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn font_id(&self, _descriptor: &Font) -> Result<FontId> {
        Ok(FontId(1))
    }

    fn font_metrics(&self, _font_id: FontId) -> FontMetrics {
        FontMetrics {
            units_per_em: 1000,
            ascent: 1025.0,
            descent: -275.0,
            line_gap: 0.0,
            underline_position: -95.0,
            underline_thickness: 60.0,
            cap_height: 698.0,
            x_height: 516.0,
            bounding_box: Bounds {
                origin: Point {
                    x: -260.0,
                    y: -245.0,
                },
                size: Size {
                    width: 1501.0,
                    height: 1364.0,
                },
            },
        }
    }

    fn typographic_bounds(&self, _font_id: FontId, _glyph_id: GlyphId) -> Result<Bounds<f32>> {
        Ok(Bounds {
            origin: Point { x: 54.0, y: 0.0 },
            size: size(392.0, 528.0),
        })
    }

    fn advance(&self, _font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        Ok(size(600.0 * glyph_id.0 as f32, 0.0))
    }

    fn glyph_for_char(&self, _font_id: FontId, ch: char) -> Option<GlyphId> {
        Some(GlyphId(ch.len_utf16() as u32))
    }

    fn glyph_raster_bounds(&self, _params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        Ok(Default::default())
    }

    fn rasterize_glyph(
        &self,
        _params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        Ok((raster_bounds.size, Vec::new()))
    }

    fn layout_line(&self, text: &str, font_size: Pixels, _runs: &[FontRun]) -> LineLayout {
        let mut position = px(0.);
        let metrics = self.font_metrics(FontId(0));
        let em_width = font_size
            * self
                .advance(FontId(0), self.glyph_for_char(FontId(0), 'm').unwrap())
                .unwrap()
                .width
            / metrics.units_per_em as f32;
        let mut glyphs = Vec::new();
        for (ix, c) in text.char_indices() {
            if let Some(glyph) = self.glyph_for_char(FontId(0), c) {
                glyphs.push(ShapedGlyph {
                    id: glyph,
                    position: point(position, px(0.)),
                    index: ix,
                    is_emoji: glyph.0 == 2,
                });
                if glyph.0 == 2 {
                    position += em_width * 2.0;
                } else {
                    position += em_width;
                }
            } else {
                position += em_width
            }
        }
        let mut runs = Vec::default();
        if !glyphs.is_empty() {
            runs.push(ShapedRun {
                font_id: FontId(0),
                glyphs,
            });
        } else {
            position = px(0.);
        }

        LineLayout {
            font_size,
            width: position,
            ascent: font_size * (metrics.ascent / metrics.units_per_em as f32),
            descent: font_size * (metrics.descent / metrics.units_per_em as f32),
            runs,
            len: text.len(),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) enum AtlasKey {
    Glyph(RenderGlyphParams),
    Svg(RenderSvgParams),
    IconAtlas(RenderIconAtlasParams),
    Image(RenderImageParams),
    CachedSurface(CachedSurfaceParams),
    Shadow(crate::shadow_cache::ShadowAtlasParams),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtlasAllocationClass {
    Shared,
    SharedSmallImage,
    DedicatedLargeImage,
}

pub(crate) const SMALL_IMAGE_ATLAS_MAX_SIZE: Size<DevicePixels> = Size {
    width: DevicePixels(128),
    height: DevicePixels(128),
};

pub(crate) const SMALL_IMAGE_ATLAS_PAGE_SIZE: Size<DevicePixels> = Size {
    width: DevicePixels(512),
    height: DevicePixels(512),
};

pub(crate) const MAX_ATLAS_TEXTURE_DIMENSION: i32 = 16_384;

#[cfg(any(target_os = "linux", test))]
pub(crate) fn safe_gpu_dimension(value: f32) -> u32 {
    if !value.is_finite() {
        return 1;
    }
    value.clamp(1.0, MAX_ATLAS_TEXTURE_DIMENSION as f32) as u32
}

pub(crate) fn catch_platform_callback<T>(
    platform: &'static str,
    name: &'static str,
    fallback: T,
    callback: impl FnOnce() -> T,
) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
        Ok(value) => value,
        Err(_) => {
            log::error!(
                "{name} callback panicked; containing panic at the {platform} platform boundary"
            );
            fallback
        }
    }
}

pub(crate) fn validate_atlas_payload(
    size: Size<DevicePixels>,
    kind: AtlasTextureKind,
    byte_len: usize,
) -> Result<usize> {
    let width = usize::try_from(size.width.0)
        .ok()
        .filter(|width| *width > 0 && *width <= MAX_ATLAS_TEXTURE_DIMENSION as usize)
        .ok_or_else(|| {
            anyhow::anyhow!("atlas width must be within 1..={MAX_ATLAS_TEXTURE_DIMENSION}")
        })?;
    let height = usize::try_from(size.height.0)
        .ok()
        .filter(|height| *height > 0 && *height <= MAX_ATLAS_TEXTURE_DIMENSION as usize)
        .ok_or_else(|| {
            anyhow::anyhow!("atlas height must be within 1..={MAX_ATLAS_TEXTURE_DIMENSION}")
        })?;
    let bytes_per_pixel = match kind {
        AtlasTextureKind::Monochrome => 1,
        AtlasTextureKind::Polychrome => 4,
    };
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
        .ok_or_else(|| anyhow::anyhow!("atlas payload size overflow"))?;
    anyhow::ensure!(
        byte_len == expected,
        "atlas payload length mismatch: expected {expected} bytes, received {byte_len}"
    );
    Ok(expected)
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) struct CachedSurfaceParams {
    pub(crate) cache_id: u64,
    pub(crate) size: Size<DevicePixels>,
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) struct RenderIconAtlasParams {
    pub(crate) edge: DevicePixels,
}

impl AtlasKey {
    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub(crate) fn texture_kind(&self) -> AtlasTextureKind {
        match self {
            AtlasKey::Glyph(params) => {
                if params.is_emoji || params.raster_mode == crate::GlyphRasterMode::Subpixel {
                    AtlasTextureKind::Polychrome
                } else {
                    AtlasTextureKind::Monochrome
                }
            }
            AtlasKey::Svg(_) => AtlasTextureKind::Monochrome,
            AtlasKey::IconAtlas(_) => AtlasTextureKind::Monochrome,
            AtlasKey::Image(_) => AtlasTextureKind::Polychrome,
            AtlasKey::CachedSurface(_) => AtlasTextureKind::Polychrome,
            AtlasKey::Shadow(_) => AtlasTextureKind::Polychrome,
        }
    }

    pub(crate) fn allocation_class(&self, size: Size<DevicePixels>) -> AtlasAllocationClass {
        match self {
            AtlasKey::IconAtlas(_) => AtlasAllocationClass::DedicatedLargeImage,
            AtlasKey::Image(_)
                if size.width.0 <= SMALL_IMAGE_ATLAS_MAX_SIZE.width.0
                    && size.height.0 <= SMALL_IMAGE_ATLAS_MAX_SIZE.height.0 =>
            {
                AtlasAllocationClass::SharedSmallImage
            }
            AtlasKey::Image(_) => AtlasAllocationClass::DedicatedLargeImage,
            _ => AtlasAllocationClass::Shared,
        }
    }
}

impl AtlasAllocationClass {
    pub(crate) fn texture_size(
        self,
        min_size: Size<DevicePixels>,
        default_size: Size<DevicePixels>,
        max_size: Size<DevicePixels>,
    ) -> Size<DevicePixels> {
        match self {
            AtlasAllocationClass::Shared => min_size.min(&max_size).max(&default_size),
            AtlasAllocationClass::SharedSmallImage => {
                min_size.min(&max_size).max(&SMALL_IMAGE_ATLAS_PAGE_SIZE)
            }
            AtlasAllocationClass::DedicatedLargeImage => min_size.min(&max_size),
        }
    }
}

impl From<RenderGlyphParams> for AtlasKey {
    fn from(params: RenderGlyphParams) -> Self {
        Self::Glyph(params)
    }
}

impl From<RenderSvgParams> for AtlasKey {
    fn from(params: RenderSvgParams) -> Self {
        Self::Svg(params)
    }
}

impl From<RenderImageParams> for AtlasKey {
    fn from(params: RenderImageParams) -> Self {
        Self::Image(params)
    }
}

impl From<CachedSurfaceParams> for AtlasKey {
    fn from(params: CachedSurfaceParams) -> Self {
        Self::CachedSurface(params)
    }
}

impl From<crate::shadow_cache::ShadowAtlasParams> for AtlasKey {
    fn from(params: crate::shadow_cache::ShadowAtlasParams) -> Self {
        Self::Shadow(params)
    }
}

pub(crate) trait PlatformAtlas: Send + Sync {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>>;
    fn remove(&self, key: &AtlasKey);
}

struct AtlasTextureList<T> {
    textures: Vec<Option<T>>,
    free_list: Vec<usize>,
}

impl<T> Default for AtlasTextureList<T> {
    fn default() -> Self {
        Self {
            textures: Vec::default(),
            free_list: Vec::default(),
        }
    }
}

impl<T> ops::Index<usize> for AtlasTextureList<T> {
    type Output = Option<T>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.textures[index]
    }
}

impl<T> AtlasTextureList<T> {
    #[allow(unused)]
    fn drain(&mut self) -> std::vec::Drain<'_, Option<T>> {
        self.free_list.clear();
        self.textures.drain(..)
    }

    #[allow(dead_code)]
    fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.textures.iter_mut().flatten()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct AtlasTile {
    pub(crate) texture_id: AtlasTextureId,
    pub(crate) tile_id: TileId,
    pub(crate) padding: u32,
    pub(crate) bounds: Bounds<DevicePixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub(crate) struct AtlasTextureId {
    // We use u32 instead of usize for Metal Shader Language compatibility
    pub(crate) index: u32,
    pub(crate) kind: AtlasTextureKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum AtlasTextureKind {
    Monochrome = 0,
    Polychrome = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub(crate) struct TileId(pub(crate) u32);

impl From<etagere::AllocId> for TileId {
    fn from(id: etagere::AllocId) -> Self {
        Self(id.serialize())
    }
}

impl From<TileId> for etagere::AllocId {
    fn from(id: TileId) -> Self {
        Self::deserialize(id.0)
    }
}

/// Choose which atlas tiles to evict to bring the atlas's total byte usage down to
/// `max_bytes`, least-recently-used first.
///
/// `tiles` lists each evictable tile as `(last_used_frame, byte_size)`. A tile last used in
/// the *current* frame is never selected: it may be referenced by the in-flight GPU render,
/// so reclaiming its atlas region could corrupt the frame. Returns indices into `tiles`,
/// oldest first; if the not-this-frame tiles cannot free enough, it returns all it safely
/// can (the atlas may remain over budget until those tiles age out — by design, never at the
/// cost of correctness).
///
/// This pure policy remains useful for callers budgeting individually accounted resources.
/// Texture-atlas backends use page-aware eviction because freeing a tile does not necessarily
/// release its shared GPU texture page.
#[allow(dead_code)]
pub(crate) fn select_atlas_evictions(
    tiles: &[(u64, u64)],
    total_bytes: u64,
    max_bytes: u64,
    current_frame: u64,
) -> Vec<usize> {
    if total_bytes <= max_bytes {
        return Vec::new();
    }

    let mut candidates: Vec<usize> = (0..tiles.len())
        .filter(|&index| tiles[index].0 < current_frame)
        .collect();
    candidates.sort_by_key(|&index| tiles[index].0);

    let needed = total_bytes - max_bytes;
    let mut freed = 0u64;
    let mut victims = Vec::new();
    for index in candidates {
        if freed >= needed {
            break;
        }
        freed = freed.saturating_add(tiles[index].1);
        victims.push(index);
    }
    victims
}

pub(crate) struct PlatformInputHandler {
    cx: AsyncWindowContext,
    handler: Box<dyn InputHandler>,
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
impl PlatformInputHandler {
    pub fn new(cx: AsyncWindowContext, handler: Box<dyn InputHandler>) -> Self {
        Self { cx, handler }
    }

    fn selected_text_range(&mut self, ignore_disabled_input: bool) -> Option<UTF16Selection> {
        self.cx
            .update(|window, cx| {
                self.handler
                    .selected_text_range(ignore_disabled_input, window, cx)
            })
            .ok()
            .flatten()
    }

    #[cfg_attr(target_os = "windows", allow(dead_code))]
    fn marked_text_range(&mut self) -> Option<Range<usize>> {
        self.cx
            .update(|window, cx| self.handler.marked_text_range(window, cx))
            .ok()
            .flatten()
    }

    #[cfg_attr(
        any(target_os = "linux", target_os = "freebsd", target_os = "windows"),
        allow(dead_code)
    )]
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
    ) -> Option<String> {
        self.cx
            .update(|window, cx| {
                self.handler
                    .text_for_range(range_utf16, adjusted, window, cx)
            })
            .ok()
            .flatten()
    }

    fn replace_text_in_range(&mut self, replacement_range: Option<Range<usize>>, text: &str) {
        self.cx
            .update(|window, cx| {
                self.handler
                    .replace_text_in_range(replacement_range, text, window, cx);
            })
            .ok();
    }

    pub fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
    ) {
        self.cx
            .update(|window, cx| {
                self.handler.replace_and_mark_text_in_range(
                    range_utf16,
                    new_text,
                    new_selected_range,
                    window,
                    cx,
                )
            })
            .ok();
    }

    #[cfg_attr(target_os = "windows", allow(dead_code))]
    fn unmark_text(&mut self) {
        self.cx
            .update(|window, cx| self.handler.unmark_text(window, cx))
            .ok();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>) -> Option<Bounds<Pixels>> {
        self.cx
            .update(|window, cx| self.handler.bounds_for_range(range_utf16, window, cx))
            .ok()
            .flatten()
    }

    #[allow(dead_code)]
    fn apple_press_and_hold_enabled(&mut self) -> bool {
        self.handler.apple_press_and_hold_enabled()
    }

    pub(crate) fn dispatch_input(&mut self, input: &str, window: &mut Window, cx: &mut App) {
        self.handler.replace_text_in_range(None, input, window, cx);
    }

    pub fn selected_bounds(&mut self, window: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
        let selection = self.handler.selected_text_range(true, window, cx)?;
        self.handler.bounds_for_range(
            if selection.reversed {
                selection.range.start..selection.range.start
            } else {
                selection.range.end..selection.range.end
            },
            window,
            cx,
        )
    }

    #[allow(unused)]
    pub fn character_index_for_point(&mut self, point: Point<Pixels>) -> Option<usize> {
        self.cx
            .update(|window, cx| self.handler.character_index_for_point(point, window, cx))
            .ok()
            .flatten()
    }
}

/// A struct representing a selection in a text buffer, in UTF16 characters.
/// This is different from a range because the head may be before the tail.
#[derive(Debug)]
pub struct UTF16Selection {
    /// The range of text in the document this selection corresponds to
    /// in UTF16 characters.
    pub range: Range<usize>,
    /// Whether the head of this selection is at the start (true), or end (false)
    /// of the range
    pub reversed: bool,
}

/// Kael's interface for handling text input from the platform's IME system
/// This is currently a 1:1 exposure of the NSTextInputClient API:
///
/// <https://developer.apple.com/documentation/appkit/nstextinputclient>
pub trait InputHandler: 'static {
    /// Get the range of the user's currently selected text, if any
    /// Corresponds to [selectedRange()](https://developer.apple.com/documentation/appkit/nstextinputclient/1438242-selectedrange)
    ///
    /// Return value is in terms of UTF-16 characters, from 0 to the length of the document
    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection>;

    /// Get the range of the currently marked text, if any
    /// Corresponds to [markedRange()](https://developer.apple.com/documentation/appkit/nstextinputclient/1438250-markedrange)
    ///
    /// Return value is in terms of UTF-16 characters, from 0 to the length of the document
    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>>;

    /// Get the text for the given document range in UTF-16 characters
    /// Corresponds to [attributedSubstring(forProposedRange: actualRange:)](https://developer.apple.com/documentation/appkit/nstextinputclient/1438238-attributedsubstring)
    ///
    /// range_utf16 is in terms of UTF-16 characters
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String>;

    /// Replace the text in the given document range with the given text
    /// Corresponds to [insertText(_:replacementRange:)](https://developer.apple.com/documentation/appkit/nstextinputclient/1438258-inserttext)
    ///
    /// replacement_range is in terms of UTF-16 characters
    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    );

    /// Replace the text in the given document range with the given text,
    /// and mark the given text as part of an IME 'composing' state
    /// Corresponds to [setMarkedText(_:selectedRange:replacementRange:)](https://developer.apple.com/documentation/appkit/nstextinputclient/1438246-setmarkedtext)
    ///
    /// range_utf16 is in terms of UTF-16 characters
    /// new_selected_range is in terms of UTF-16 characters
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    );

    /// Remove the IME 'composing' state from the document
    /// Corresponds to [unmarkText()](https://developer.apple.com/documentation/appkit/nstextinputclient/1438239-unmarktext)
    fn unmark_text(&mut self, window: &mut Window, cx: &mut App);

    /// Get the bounds of the given document range in screen coordinates
    /// Corresponds to [firstRect(forCharacterRange:actualRange:)](https://developer.apple.com/documentation/appkit/nstextinputclient/1438240-firstrect)
    ///
    /// This is used for positioning the IME candidate window
    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>>;

    /// Get the character offset for the given point in terms of UTF16 characters
    ///
    /// Corresponds to [characterIndexForPoint:](https://developer.apple.com/documentation/appkit/nstextinputclient/characterindex(for:))
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize>;

    /// Allows a given input context to opt into getting raw key repeats instead of
    /// sending these to the platform.
    /// TODO: Ideally we should be able to set ApplePressAndHoldEnabled in NSUserDefaults
    /// (which is how iTerm does it) but it doesn't seem to work for me.
    #[allow(dead_code)]
    fn apple_press_and_hold_enabled(&mut self) -> bool {
        true
    }
}

/// The variables that can be configured when creating a new window
#[derive(Debug)]
pub struct WindowOptions {
    /// Specifies the state and bounds of the window in screen coordinates.
    /// - `None`: Inherit the bounds.
    /// - `Some(WindowBounds)`: Open a window with corresponding state and its restore size.
    pub window_bounds: Option<WindowBounds>,

    /// The titlebar configuration of the window
    pub titlebar: Option<TitlebarOptions>,

    /// Whether the window should be focused when created
    pub focus: bool,

    /// Whether the window should be shown when created
    pub show: bool,

    /// The kind of window to create
    pub kind: WindowKind,

    /// Whether the window should be movable by the user
    pub is_movable: bool,

    /// Whether the window should be resizable by the user
    pub is_resizable: bool,

    /// Whether the window should be minimized by the user
    pub is_minimizable: bool,

    /// The display to create the window on, if this is None,
    /// the window will be created on the main display
    pub display_id: Option<DisplayId>,

    /// The appearance of the window background.
    pub window_background: WindowBackgroundAppearance,

    /// Application identifier of the window. Can by used by desktop environments to group applications together.
    pub app_id: Option<String>,

    /// Window minimum size
    pub window_min_size: Option<Size<Pixels>>,

    /// Whether to use client or server side decorations. Wayland only
    /// Note that this may be ignored.
    pub window_decorations: Option<WindowDecorations>,

    /// Tab group name, allows opening the window as a native tab on macOS 10.12+. Windows with the same tabbing identifier will be grouped together.
    ///
    /// macOS-only: native window tabbing is an AppKit feature. On Windows and Linux this
    /// field is accepted but ignored (no native tab grouping exists).
    pub tabbing_identifier: Option<String>,

    /// Whether the window should allow mouse events to pass through to windows behind it
    pub mouse_passthrough: bool,

    /// The parent window handle for creating child windows.
    /// When set, the new window will be created as a child of the specified parent.
    pub parent: Option<AnyWindowHandle>,
}

/// The variables that can be configured when creating a new window
#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) struct WindowParams {
    pub bounds: Bounds<Pixels>,

    /// The titlebar configuration of the window
    #[cfg_attr(feature = "wayland", allow(dead_code))]
    pub titlebar: Option<TitlebarOptions>,

    /// The kind of window to create
    #[cfg_attr(any(target_os = "linux", target_os = "freebsd"), allow(dead_code))]
    pub kind: WindowKind,

    /// Whether the window should be movable by the user
    #[cfg_attr(any(target_os = "linux", target_os = "freebsd"), allow(dead_code))]
    pub is_movable: bool,

    /// Whether the window should be resizable by the user
    #[cfg_attr(any(target_os = "linux", target_os = "freebsd"), allow(dead_code))]
    pub is_resizable: bool,

    /// Whether the window should be minimized by the user
    #[cfg_attr(any(target_os = "linux", target_os = "freebsd"), allow(dead_code))]
    pub is_minimizable: bool,

    #[cfg_attr(
        any(target_os = "linux", target_os = "freebsd", target_os = "windows"),
        allow(dead_code)
    )]
    pub focus: bool,

    #[cfg_attr(any(target_os = "linux", target_os = "freebsd"), allow(dead_code))]
    pub show: bool,

    #[cfg_attr(feature = "wayland", allow(dead_code))]
    pub display_id: Option<DisplayId>,

    pub window_min_size: Option<Size<Pixels>>,
    #[cfg(target_os = "macos")]
    pub tabbing_identifier: Option<String>,

    #[allow(dead_code)]
    pub mouse_passthrough: bool,

    /// The parent window handle for creating child windows.
    pub parent: Option<AnyWindowHandle>,
}

/// Represents the status of how a window should be opened.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub enum WindowBounds {
    /// Indicates that the window should open in a windowed state with the given bounds.
    Windowed(Bounds<Pixels>),
    /// Indicates that the window should open in a maximized state.
    /// The bounds provided here represent the restore size of the window.
    Maximized(Bounds<Pixels>),
    /// Indicates that the window should open in fullscreen mode.
    /// The bounds provided here represent the restore size of the window.
    Fullscreen(Bounds<Pixels>),
}

impl Default for WindowBounds {
    fn default() -> Self {
        WindowBounds::Windowed(Bounds::default())
    }
}

impl WindowBounds {
    /// Retrieve the inner bounds
    pub fn get_bounds(&self) -> Bounds<Pixels> {
        match self {
            WindowBounds::Windowed(bounds) => *bounds,
            WindowBounds::Maximized(bounds) => *bounds,
            WindowBounds::Fullscreen(bounds) => *bounds,
        }
    }

    /// Creates a new window bounds that centers the window on the screen.
    pub fn centered(size: Size<Pixels>, cx: &App) -> Self {
        WindowBounds::Windowed(Bounds::centered(None, size, cx))
    }
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            window_bounds: None,
            titlebar: Some(TitlebarOptions {
                title: Default::default(),
                appears_transparent: Default::default(),
                traffic_light_position: Default::default(),
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            is_resizable: true,
            is_minimizable: true,
            display_id: None,
            window_background: WindowBackgroundAppearance::default(),
            app_id: None,
            window_min_size: None,
            window_decorations: None,
            tabbing_identifier: None,
            mouse_passthrough: false,
            parent: None,
        }
    }
}

impl WindowOptions {
    /// Whether explicit bounds were configured.
    pub fn has_bounds(&self) -> bool {
        self.window_bounds.is_some()
    }

    /// Content-safe bounds mode without exposing screen coordinates.
    pub fn bounds_mode(&self) -> &'static str {
        match self.window_bounds {
            Some(WindowBounds::Windowed(_)) => "windowed",
            Some(WindowBounds::Maximized(_)) => "maximized",
            Some(WindowBounds::Fullscreen(_)) => "fullscreen",
            None => "inherited",
        }
    }

    /// Whether a titlebar configuration is present.
    pub fn has_titlebar(&self) -> bool {
        self.titlebar.is_some()
    }

    /// Whether a title is configured without exposing its value.
    pub fn has_title(&self) -> bool {
        self.titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.as_ref())
            .is_some()
    }

    /// Whether the titlebar is configured as transparent/custom chrome.
    pub fn has_transparent_titlebar(&self) -> bool {
        self.titlebar
            .as_ref()
            .is_some_and(|titlebar| titlebar.appears_transparent)
    }

    /// Whether macOS traffic-light positioning is configured.
    pub fn has_traffic_light_position(&self) -> bool {
        self.titlebar
            .as_ref()
            .is_some_and(|titlebar| titlebar.traffic_light_position.is_some())
    }

    /// Whether the window is hidden on creation.
    pub fn starts_hidden(&self) -> bool {
        !self.show
    }

    /// Whether the window is unfocused on creation.
    pub fn starts_unfocused(&self) -> bool {
        !self.focus
    }

    /// Whether the window has a fixed size from the user's perspective.
    pub fn fixed_size(&self) -> bool {
        !self.is_resizable
    }

    /// Whether a display target is configured.
    pub fn has_display_id(&self) -> bool {
        self.display_id.is_some()
    }

    /// Whether an application grouping id is configured.
    pub fn has_app_id(&self) -> bool {
        self.app_id.is_some()
    }

    /// Whether a native tabbing identifier is configured.
    pub fn has_tabbing_identifier(&self) -> bool {
        self.tabbing_identifier.is_some()
    }

    /// Whether a parent window is configured.
    pub fn has_parent(&self) -> bool {
        self.parent.is_some()
    }

    /// Whether a minimum size is configured.
    pub fn has_min_size(&self) -> bool {
        self.window_min_size.is_some()
    }

    /// Whether client-side decorations are requested.
    pub fn uses_client_decorations(&self) -> bool {
        self.window_decorations == Some(WindowDecorations::Client)
    }

    /// Content-safe summary for window-management options.
    pub fn to_text(&self) -> String {
        let titlebar = if let Some(titlebar) = &self.titlebar {
            if titlebar.appears_transparent {
                "transparent"
            } else {
                "system"
            }
        } else {
            "none"
        };
        let decorations = self
            .window_decorations
            .map(WindowDecorations::to_text)
            .unwrap_or("default");

        format!(
            "window options: kind {}, bounds {}, background {}, titlebar {}, decorations {}, title {}, min-size {}, display {}, app-id {}, tabbing {}, parent {}, shown {}, focused {}, movable {}, resizable {}, minimizable {}, mouse-passthrough {}",
            self.kind.to_text(),
            self.bounds_mode(),
            self.window_background.to_text(),
            titlebar,
            decorations,
            bool_text(self.has_title()),
            bool_text(self.has_min_size()),
            bool_text(self.has_display_id()),
            bool_text(self.has_app_id()),
            bool_text(self.has_tabbing_identifier()),
            bool_text(self.has_parent()),
            bool_text(self.show),
            bool_text(self.focus),
            bool_text(self.is_movable),
            bool_text(self.is_resizable),
            bool_text(self.is_minimizable),
            bool_text(self.mouse_passthrough),
        )
    }
}

/// Builder for [`WindowOptions`].
#[derive(Debug)]
pub struct WindowOptionsBuilder {
    options: WindowOptions,
}

impl WindowOptionsBuilder {
    /// Create a builder with [`WindowOptions::default`] values.
    pub fn new() -> Self {
        Self {
            options: WindowOptions::default(),
        }
    }

    /// Start from an existing raw options value.
    pub fn from_options(options: WindowOptions) -> Self {
        Self { options }
    }

    /// Set the window bounds/state.
    pub fn bounds(mut self, bounds: WindowBounds) -> Self {
        self.options.window_bounds = Some(bounds);
        self
    }

    /// Open as a normal window using the given screen bounds.
    pub fn windowed(self, bounds: Bounds<Pixels>) -> Self {
        self.bounds(WindowBounds::Windowed(bounds))
    }

    /// Open maximized while preserving the given restore bounds.
    pub fn maximized(self, restore_bounds: Bounds<Pixels>) -> Self {
        self.bounds(WindowBounds::Maximized(restore_bounds))
    }

    /// Open fullscreen while preserving the given restore bounds.
    pub fn fullscreen(self, restore_bounds: Bounds<Pixels>) -> Self {
        self.bounds(WindowBounds::Fullscreen(restore_bounds))
    }

    /// Open centered on the primary display.
    pub fn centered(self, size: Size<Pixels>, cx: &App) -> Self {
        self.bounds(WindowBounds::centered(size, cx))
    }

    /// Open using resolved monitor-aware placement bounds.
    pub fn placement(self, placement: &WindowPlacement) -> Self {
        let builder = self.windowed(placement.bounds());
        if let Some(display_id) = placement.display_id() {
            builder.display_id(display_id)
        } else {
            builder
        }
    }

    /// Set the initial window title.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.ensure_titlebar().title = Some(title.into());
        self
    }

    /// Replace the entire titlebar configuration.
    pub fn titlebar(mut self, titlebar: Option<TitlebarOptions>) -> Self {
        self.options.titlebar = titlebar;
        self
    }

    /// Hide the default system titlebar where the platform supports it.
    pub fn transparent_titlebar(mut self, transparent: bool) -> Self {
        self.ensure_titlebar().appears_transparent = transparent;
        self
    }

    /// Remove the titlebar configuration.
    pub fn no_titlebar(mut self) -> Self {
        self.options.titlebar = None;
        self
    }

    /// Set the macOS traffic-light position.
    pub fn traffic_light_position(mut self, position: Point<Pixels>) -> Self {
        self.ensure_titlebar().traffic_light_position = Some(position);
        self
    }

    /// Set whether the window should be focused when created.
    pub fn focused(mut self, focus: bool) -> Self {
        self.options.focus = focus;
        self
    }

    /// Create the window without focusing it.
    pub fn unfocused(self) -> Self {
        self.focused(false)
    }

    /// Set whether the window should be shown when created.
    pub fn show(mut self, show: bool) -> Self {
        self.options.show = show;
        self
    }

    /// Create the window hidden.
    pub fn hidden(self) -> Self {
        self.show(false)
    }

    /// Set the platform window kind.
    pub fn kind(mut self, kind: WindowKind) -> Self {
        self.options.kind = kind;
        self
    }

    /// Create a pop-up window.
    pub fn popup(self) -> Self {
        self.kind(WindowKind::PopUp)
    }

    /// Create a floating window.
    pub fn floating(self) -> Self {
        self.kind(WindowKind::Floating)
    }

    /// Create an overlay window.
    pub fn overlay(self) -> Self {
        self.kind(WindowKind::Overlay)
    }

    /// Set whether the window is movable by the user.
    pub fn movable(mut self, movable: bool) -> Self {
        self.options.is_movable = movable;
        self
    }

    /// Set whether the window is resizable by the user.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.options.is_resizable = resizable;
        self
    }

    /// Set whether the window is minimizable by the user.
    pub fn minimizable(mut self, minimizable: bool) -> Self {
        self.options.is_minimizable = minimizable;
        self
    }

    /// Pin window creation to a display.
    pub fn display_id(mut self, display_id: DisplayId) -> Self {
        self.options.display_id = Some(display_id);
        self
    }

    /// Set the window background appearance.
    pub fn background(mut self, background: WindowBackgroundAppearance) -> Self {
        self.options.window_background = background;
        self
    }

    /// Use a transparent window background.
    pub fn transparent_background(self) -> Self {
        self.background(WindowBackgroundAppearance::Transparent)
    }

    /// Use a blurred window background when the platform supports it.
    pub fn blurred_background(self) -> Self {
        self.background(WindowBackgroundAppearance::Blurred)
    }

    /// Set the desktop application identifier used for grouping on some DEs.
    pub fn app_id(mut self, app_id: impl Into<String>) -> Self {
        self.options.app_id = Some(app_id.into());
        self
    }

    /// Set the minimum window size.
    pub fn min_size(mut self, size: Size<Pixels>) -> Self {
        self.options.window_min_size = Some(size);
        self
    }

    /// Set the window decoration preference.
    pub fn decorations(mut self, decorations: WindowDecorations) -> Self {
        self.options.window_decorations = Some(decorations);
        self
    }

    /// Prefer client-side decorations.
    pub fn client_decorations(self) -> Self {
        self.decorations(WindowDecorations::Client)
    }

    /// Prefer server-side decorations.
    pub fn server_decorations(self) -> Self {
        self.decorations(WindowDecorations::Server)
    }

    /// Set the native tab group identifier.
    pub fn tabbing_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.options.tabbing_identifier = Some(identifier.into());
        self
    }

    /// Set whether mouse events pass through to windows behind this one.
    pub fn mouse_passthrough(mut self, passthrough: bool) -> Self {
        self.options.mouse_passthrough = passthrough;
        self
    }

    /// Set the parent window for child-window creation.
    pub fn parent(mut self, parent: AnyWindowHandle) -> Self {
        self.options.parent = Some(parent);
        self
    }

    /// Whether explicit bounds are configured.
    pub fn has_bounds(&self) -> bool {
        self.options.has_bounds()
    }

    /// Content-safe bounds mode without exposing screen coordinates.
    pub fn bounds_mode(&self) -> &'static str {
        self.options.bounds_mode()
    }

    /// Whether a titlebar configuration is present.
    pub fn has_titlebar(&self) -> bool {
        self.options.has_titlebar()
    }

    /// Whether a title is configured without exposing its value.
    pub fn has_title(&self) -> bool {
        self.options.has_title()
    }

    /// Whether the titlebar is configured as transparent/custom chrome.
    pub fn has_transparent_titlebar(&self) -> bool {
        self.options.has_transparent_titlebar()
    }

    /// Whether macOS traffic-light positioning is configured.
    pub fn has_traffic_light_position(&self) -> bool {
        self.options.has_traffic_light_position()
    }

    /// Whether the window is hidden on creation.
    pub fn starts_hidden(&self) -> bool {
        self.options.starts_hidden()
    }

    /// Whether the window is unfocused on creation.
    pub fn starts_unfocused(&self) -> bool {
        self.options.starts_unfocused()
    }

    /// Whether the window has a fixed size from the user's perspective.
    pub fn fixed_size(&self) -> bool {
        self.options.fixed_size()
    }

    /// Whether a display target is configured.
    pub fn has_display_id(&self) -> bool {
        self.options.has_display_id()
    }

    /// Whether an application grouping id is configured.
    pub fn has_app_id(&self) -> bool {
        self.options.has_app_id()
    }

    /// Whether a native tabbing identifier is configured.
    pub fn has_tabbing_identifier(&self) -> bool {
        self.options.has_tabbing_identifier()
    }

    /// Whether a parent window is configured.
    pub fn has_parent(&self) -> bool {
        self.options.has_parent()
    }

    /// Whether a minimum size is configured.
    pub fn has_min_size(&self) -> bool {
        self.options.has_min_size()
    }

    /// Whether client-side decorations are requested.
    pub fn uses_client_decorations(&self) -> bool {
        self.options.uses_client_decorations()
    }

    /// Content-safe summary for window-management options before creation.
    pub fn to_text(&self) -> String {
        self.options.to_text()
    }

    /// Consume the builder into raw window options.
    pub fn build(self) -> WindowOptions {
        self.options
    }

    fn ensure_titlebar(&mut self) -> &mut TitlebarOptions {
        self.options.titlebar.get_or_insert_with(Default::default)
    }
}

impl Default for WindowOptionsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<WindowOptionsBuilder> for WindowOptions {
    fn from(value: WindowOptionsBuilder) -> Self {
        value.build()
    }
}

/// High-level intent for a native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowIntentKind {
    /// Main application/document window.
    Main,
    /// Command palette, launcher, or transient search window.
    Palette,
    /// Tool/inspector/sidebar utility window.
    Utility,
    /// Child modal or dialog-like window.
    Modal,
    /// Context popup or short-lived popover window.
    Popup,
    /// Overlay/HUD window.
    Overlay,
}

impl WindowIntentKind {
    /// Stable label for diagnostics and generated UI.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Palette => "palette",
            Self::Utility => "utility",
            Self::Modal => "modal",
            Self::Popup => "popup",
            Self::Overlay => "overlay",
        }
    }
}

/// Checked builder for window-management window intent presets.
#[derive(Debug)]
pub struct WindowIntentBuilder {
    kind: WindowIntentKind,
    options: WindowOptionsBuilder,
}

impl WindowIntentBuilder {
    /// Create a window intent from a kind.
    pub fn new(kind: WindowIntentKind) -> Self {
        Self {
            kind,
            options: window_intent_defaults(kind),
        }
    }

    /// Main application/document window intent.
    pub fn main() -> Self {
        Self::new(WindowIntentKind::Main)
    }

    /// Command palette or launcher intent.
    pub fn palette() -> Self {
        Self::new(WindowIntentKind::Palette)
    }

    /// Tool, inspector, or utility window intent.
    pub fn utility() -> Self {
        Self::new(WindowIntentKind::Utility)
    }

    /// Modal/child window intent.
    pub fn modal(parent: AnyWindowHandle) -> Self {
        Self::new(WindowIntentKind::Modal).parent(parent)
    }

    /// Context popup/popover intent.
    pub fn popup() -> Self {
        Self::new(WindowIntentKind::Popup)
    }

    /// Overlay/HUD intent.
    pub fn overlay() -> Self {
        Self::new(WindowIntentKind::Overlay)
    }

    /// Return this intent kind.
    pub fn kind(&self) -> WindowIntentKind {
        self.kind
    }

    /// Content-safe window option summary for this intent.
    pub fn options_summary(&self) -> String {
        self.options.to_text()
    }

    /// Whether the intent configures custom/transparent titlebar chrome.
    pub fn has_transparent_titlebar(&self) -> bool {
        self.options.has_transparent_titlebar()
    }

    /// Whether the intent opens hidden.
    pub fn starts_hidden(&self) -> bool {
        self.options.starts_hidden()
    }

    /// Whether the intent opens unfocused.
    pub fn starts_unfocused(&self) -> bool {
        self.options.starts_unfocused()
    }

    /// Whether the intent configures a parent window.
    pub fn has_parent(&self) -> bool {
        self.options.has_parent()
    }

    /// Whether the intent configures explicit bounds.
    pub fn has_bounds(&self) -> bool {
        self.options.has_bounds()
    }

    /// Content-safe summary for traces, generated UI, and agent audits.
    pub fn to_text(&self) -> String {
        format!(
            "window intent {}: {}",
            self.kind.to_text(),
            self.options_summary()
        )
    }

    /// Refine the underlying window options builder.
    pub fn configure(
        mut self,
        configure: impl FnOnce(WindowOptionsBuilder) -> WindowOptionsBuilder,
    ) -> Self {
        self.options = configure(self.options);
        self
    }

    /// Set initial title.
    pub fn title(self, title: impl Into<SharedString>) -> Self {
        self.configure(|options| options.title(title))
    }

    /// Set explicit bounds.
    pub fn windowed(self, bounds: Bounds<Pixels>) -> Self {
        self.configure(|options| options.windowed(bounds))
    }

    /// Apply resolved monitor-aware placement.
    pub fn placement(self, placement: &WindowPlacement) -> Self {
        self.configure(|options| options.placement(placement))
    }

    /// Set minimum size.
    pub fn min_size(self, size: Size<Pixels>) -> Self {
        self.configure(|options| options.min_size(size))
    }

    /// Override focus behavior.
    pub fn focused(self, focus: bool) -> Self {
        self.configure(|options| options.focused(focus))
    }

    /// Create the window hidden.
    pub fn hidden(self) -> Self {
        self.configure(WindowOptionsBuilder::hidden)
    }

    /// Set parent window for modal/child windows.
    pub fn parent(self, parent: AnyWindowHandle) -> Self {
        self.configure(|options| options.parent(parent))
    }

    /// Override app id.
    pub fn app_id(self, app_id: impl Into<String>) -> Self {
        self.configure(|options| options.app_id(app_id))
    }

    /// Validate the intent and composed options.
    pub fn validate(&self) -> anyhow::Result<()> {
        let options = &self.options.options;
        if let Some(bounds) = options.window_bounds {
            validate_window_bounds(bounds)?;
        }
        if let Some(min_size) = options.window_min_size {
            validate_window_size(min_size, "window minimum size")?;
        }
        if let Some(titlebar) = &options.titlebar
            && let Some(title) = &titlebar.title
        {
            validate_window_text(title.as_ref(), "window title", 512)?;
        }
        if let Some(app_id) = &options.app_id {
            validate_window_text(app_id, "window app id", 128)?;
            anyhow::ensure!(
                !app_id.contains('/') && !app_id.contains('\\'),
                "window app id cannot contain path separators"
            );
        }
        if let Some(identifier) = &options.tabbing_identifier {
            validate_window_text(identifier, "window tabbing identifier", 128)?;
        }

        match self.kind {
            WindowIntentKind::Main => {
                anyhow::ensure!(
                    options.kind == WindowKind::Normal,
                    "main window intent must use a normal window kind"
                );
                anyhow::ensure!(options.is_resizable, "main window intent must be resizable");
            }
            WindowIntentKind::Palette => {
                anyhow::ensure!(
                    matches!(options.kind, WindowKind::Floating | WindowKind::PopUp),
                    "palette window intent must use floating or popup kind"
                );
                anyhow::ensure!(
                    !options.is_minimizable,
                    "palette window intent should not be minimizable"
                );
            }
            WindowIntentKind::Utility => {
                anyhow::ensure!(
                    matches!(options.kind, WindowKind::Floating | WindowKind::Normal),
                    "utility window intent must use floating or normal kind"
                );
            }
            WindowIntentKind::Modal => {
                anyhow::ensure!(
                    options.parent.is_some(),
                    "modal window intent requires a parent window"
                );
                anyhow::ensure!(
                    matches!(options.kind, WindowKind::Floating | WindowKind::PopUp),
                    "modal window intent must use floating or popup kind"
                );
                anyhow::ensure!(
                    !options.is_minimizable,
                    "modal window intent should not be minimizable"
                );
            }
            WindowIntentKind::Popup => {
                anyhow::ensure!(
                    options.kind == WindowKind::PopUp,
                    "popup window intent must use popup kind"
                );
                anyhow::ensure!(
                    !options.is_resizable,
                    "popup window intent should not be resizable"
                );
            }
            WindowIntentKind::Overlay => {
                anyhow::ensure!(
                    options.kind == WindowKind::Overlay,
                    "overlay window intent must use overlay kind"
                );
                anyhow::ensure!(
                    !options.is_minimizable,
                    "overlay window intent should not be minimizable"
                );
            }
        }

        Ok(())
    }

    /// Build checked raw window options.
    pub fn build_checked(self) -> anyhow::Result<WindowOptions> {
        self.validate()?;
        Ok(self.options.build())
    }
}

fn window_intent_defaults(kind: WindowIntentKind) -> WindowOptionsBuilder {
    match kind {
        WindowIntentKind::Main => WindowOptionsBuilder::new(),
        WindowIntentKind::Palette => WindowOptionsBuilder::new()
            .floating()
            .resizable(false)
            .minimizable(false)
            .transparent_titlebar(true)
            .client_decorations(),
        WindowIntentKind::Utility => WindowOptionsBuilder::new()
            .floating()
            .transparent_titlebar(true)
            .client_decorations(),
        WindowIntentKind::Modal => WindowOptionsBuilder::new()
            .floating()
            .resizable(false)
            .minimizable(false)
            .client_decorations(),
        WindowIntentKind::Popup => WindowOptionsBuilder::new()
            .popup()
            .resizable(false)
            .minimizable(false)
            .movable(false)
            .client_decorations(),
        WindowIntentKind::Overlay => WindowOptionsBuilder::new()
            .overlay()
            .resizable(false)
            .minimizable(false)
            .movable(false)
            .transparent_background()
            .no_titlebar(),
    }
}

fn validate_window_bounds(bounds: WindowBounds) -> anyhow::Result<()> {
    validate_window_rect(bounds.get_bounds(), "window bounds")
}

fn validate_window_rect(bounds: Bounds<Pixels>, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        bounds.origin.x.0.is_finite()
            && bounds.origin.y.0.is_finite()
            && bounds.size.width.0.is_finite()
            && bounds.size.height.0.is_finite(),
        "{label} must use finite values"
    );
    validate_window_size(bounds.size, label)
}

fn validate_window_size(size: Size<Pixels>, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        size.width.0.is_finite() && size.height.0.is_finite(),
        "{label} size must use finite values"
    );
    anyhow::ensure!(
        size.width.0 > 0.0 && size.height.0 > 0.0,
        "{label} size must be greater than zero"
    );
    Ok(())
}

fn validate_window_text(value: &str, label: &str, max_len: usize) -> anyhow::Result<()> {
    anyhow::ensure!(!value.is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value.trim() == value,
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(value.len() <= max_len, "{label} is too long");
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

/// The options that can be configured for a window's titlebar
#[derive(Debug, Default)]
pub struct TitlebarOptions {
    /// The initial title of the window
    pub title: Option<SharedString>,

    /// Should the default system titlebar be hidden to allow for a custom-drawn titlebar? (macOS and Windows only)
    /// Refer to [`WindowOptions::window_decorations`] on Linux
    pub appears_transparent: bool,

    /// The position of the macOS traffic light buttons.
    ///
    /// macOS-only: there are no traffic-light buttons on Windows or Linux, so this field
    /// is accepted but ignored on those platforms.
    pub traffic_light_position: Option<Point<Pixels>>,
}

impl TitlebarOptions {
    /// Whether a title is configured without exposing it.
    pub fn has_title(&self) -> bool {
        self.title.is_some()
    }

    /// Whether the default titlebar should appear transparent for custom chrome.
    pub fn is_transparent(&self) -> bool {
        self.appears_transparent
    }

    /// Whether a macOS traffic-light position is configured.
    pub fn has_traffic_light_position(&self) -> bool {
        self.traffic_light_position.is_some()
    }

    /// Content-safe summary for generated native titlebar/custom-chrome setup.
    pub fn to_text(&self) -> String {
        format!(
            "titlebar options: title {}, transparent {}, traffic-light-position {}",
            bool_text(self.has_title()),
            bool_text(self.is_transparent()),
            bool_text(self.has_traffic_light_position())
        )
    }
}

/// The kind of window to create
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// A normal application window
    Normal,

    /// A window that appears above all other windows, usually used for alerts or popups
    /// use sparingly!
    PopUp,

    /// A floating window that appears on top of its parent window
    Floating,

    /// An overlay window that appears above all other windows, including fullscreen apps
    Overlay,
}

impl WindowKind {
    /// Stable label for diagnostics and generated UI.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::PopUp => "popup",
            Self::Floating => "floating",
            Self::Overlay => "overlay",
        }
    }
}

/// The appearance of the window, as defined by the operating system.
///
/// On macOS, this corresponds to named [`NSAppearance`](https://developer.apple.com/documentation/appkit/nsappearance)
/// values.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowAppearance {
    /// A light appearance.
    ///
    /// On macOS, this corresponds to the `aqua` appearance.
    #[default]
    Light,

    /// A light appearance with vibrant colors.
    ///
    /// On macOS, this corresponds to the `NSAppearanceNameVibrantLight` appearance.
    VibrantLight,

    /// A dark appearance.
    ///
    /// On macOS, this corresponds to the `darkAqua` appearance.
    Dark,

    /// A dark appearance with vibrant colors.
    ///
    /// On macOS, this corresponds to the `NSAppearanceNameVibrantDark` appearance.
    VibrantDark,
}

impl WindowAppearance {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::VibrantLight => "vibrant light",
            Self::Dark => "dark",
            Self::VibrantDark => "vibrant dark",
        }
    }

    /// Return true when the appearance is dark.
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark | Self::VibrantDark)
    }

    /// Return true when the appearance is light.
    pub fn is_light(self) -> bool {
        matches!(self, Self::Light | Self::VibrantLight)
    }

    /// Return true when the appearance uses platform vibrancy/material effects.
    pub fn is_vibrant(self) -> bool {
        matches!(self, Self::VibrantLight | Self::VibrantDark)
    }
}

/// The appearance of the background of the window itself, when there is
/// no content or the content is transparent.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum WindowBackgroundAppearance {
    /// Opaque.
    ///
    /// This lets the window manager know that content behind this
    /// window does not need to be drawn.
    ///
    /// Actual color depends on the system and themes should define a fully
    /// opaque background color instead.
    #[default]
    Opaque,
    /// Plain alpha transparency.
    Transparent,
    /// Transparency, but the contents behind the window are blurred.
    ///
    /// Not always supported.
    Blurred,
}

impl WindowBackgroundAppearance {
    /// Stable label for diagnostics and generated UI.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::Transparent => "transparent",
            Self::Blurred => "blurred",
        }
    }
}

fn bool_text(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// Events that can occur on a system tray icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayIconEvent {
    /// The user left-clicked the tray icon.
    LeftClick,
    /// The user right-clicked the tray icon.
    RightClick,
    /// The user double-clicked the tray icon.
    DoubleClick,
}

/// A menu item for a system tray context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// A clickable action item.
    Action {
        /// The display label.
        label: SharedString,
        /// A unique identifier for this action.
        id: SharedString,
    },
    /// A visual separator between menu items.
    Separator,
    /// A submenu containing nested items.
    Submenu {
        /// The display label.
        label: SharedString,
        /// The nested menu items.
        items: Vec<TrayMenuItem>,
    },
    /// A toggleable menu item with a checkmark.
    Toggle {
        /// The display label.
        label: SharedString,
        /// Whether the item is currently checked.
        checked: bool,
        /// A unique identifier for this toggle.
        id: SharedString,
    },
}

impl TrayMenuItem {
    /// Create a clickable tray menu action.
    pub fn action(label: impl Into<SharedString>, id: impl Into<SharedString>) -> Self {
        Self::Action {
            label: label.into(),
            id: id.into(),
        }
    }

    /// Create a visual tray menu separator.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Create a nested tray submenu.
    pub fn submenu(label: impl Into<SharedString>, items: impl Into<Vec<TrayMenuItem>>) -> Self {
        Self::Submenu {
            label: label.into(),
            items: items.into(),
        }
    }

    /// Create a toggleable tray menu item.
    pub fn toggle(
        label: impl Into<SharedString>,
        checked: bool,
        id: impl Into<SharedString>,
    ) -> Self {
        Self::Toggle {
            label: label.into(),
            checked,
            id: id.into(),
        }
    }

    /// Validate a native tray/context menu item tree.
    pub fn validate_items(items: &[TrayMenuItem]) -> Result<()> {
        validate_tray_menu_items(items)
    }

    /// Count all items in a native tray/context menu tree.
    pub fn item_count(items: &[TrayMenuItem]) -> usize {
        tray_menu_counts(items).items
    }

    /// Count action items in a native tray/context menu tree.
    pub fn action_count(items: &[TrayMenuItem]) -> usize {
        tray_menu_counts(items).actions
    }

    /// Count toggle items in a native tray/context menu tree.
    pub fn toggle_count(items: &[TrayMenuItem]) -> usize {
        tray_menu_counts(items).toggles
    }

    /// Count submenu items in a native tray/context menu tree.
    pub fn submenu_count(items: &[TrayMenuItem]) -> usize {
        tray_menu_counts(items).submenus
    }

    /// Count separators in a native tray/context menu tree.
    pub fn separator_count(items: &[TrayMenuItem]) -> usize {
        tray_menu_counts(items).separators
    }

    /// Count checked toggle items in a native tray/context menu tree.
    pub fn checked_toggle_count(items: &[TrayMenuItem]) -> usize {
        tray_menu_counts(items).checked_toggles
    }

    /// Content-safe summary for a native tray/context menu tree.
    pub fn items_to_text(items: &[TrayMenuItem]) -> String {
        tray_menu_counts(items).to_text("menu items")
    }
}

/// Builder for a system tray menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayMenuBuilder {
    items: Vec<TrayMenuItem>,
}

impl TrayMenuBuilder {
    /// Create an empty tray menu builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a clickable action item.
    pub fn action(mut self, label: impl Into<SharedString>, id: impl Into<SharedString>) -> Self {
        self.items.push(TrayMenuItem::action(label, id));
        self
    }

    /// Add a visual separator.
    pub fn separator(mut self) -> Self {
        self.items.push(TrayMenuItem::separator());
        self
    }

    /// Add a nested submenu.
    pub fn submenu(
        mut self,
        label: impl Into<SharedString>,
        items: impl Into<Vec<TrayMenuItem>>,
    ) -> Self {
        self.items.push(TrayMenuItem::submenu(label, items));
        self
    }

    /// Add a toggleable menu item.
    pub fn toggle(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        id: impl Into<SharedString>,
    ) -> Self {
        self.items.push(TrayMenuItem::toggle(label, checked, id));
        self
    }

    /// Return the configured tray menu items.
    pub fn items(&self) -> &[TrayMenuItem] {
        &self.items
    }

    /// Count all configured items, including submenu children.
    pub fn item_count(&self) -> usize {
        TrayMenuItem::item_count(&self.items)
    }

    /// Count action items.
    pub fn action_count(&self) -> usize {
        TrayMenuItem::action_count(&self.items)
    }

    /// Count toggle items.
    pub fn toggle_count(&self) -> usize {
        TrayMenuItem::toggle_count(&self.items)
    }

    /// Count submenu items.
    pub fn submenu_count(&self) -> usize {
        TrayMenuItem::submenu_count(&self.items)
    }

    /// Count separators.
    pub fn separator_count(&self) -> usize {
        TrayMenuItem::separator_count(&self.items)
    }

    /// Count checked toggle items.
    pub fn checked_toggle_count(&self) -> usize {
        TrayMenuItem::checked_toggle_count(&self.items)
    }

    /// Content-safe summary that avoids labels and action IDs.
    pub fn to_text(&self) -> String {
        tray_menu_counts(&self.items).to_text("tray menu")
    }

    /// Validate labels and action IDs before installing the menu.
    pub fn validate(&self) -> Result<()> {
        validate_tray_menu_items(&self.items)
    }

    /// Build the validated tray menu item tree.
    pub fn build(self) -> Result<Vec<TrayMenuItem>> {
        self.validate()?;
        Ok(self.items)
    }

    /// Consume the builder into tray menu items.
    pub fn into_items(self) -> Vec<TrayMenuItem> {
        self.items
    }
}

impl From<TrayMenuBuilder> for Vec<TrayMenuItem> {
    fn from(value: TrayMenuBuilder) -> Self {
        value.into_items()
    }
}

/// Builder for a native context menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeContextMenuBuilder {
    items: Vec<TrayMenuItem>,
}

impl NativeContextMenuBuilder {
    /// Create an empty context menu builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a clickable action item.
    pub fn action(mut self, label: impl Into<SharedString>, id: impl Into<SharedString>) -> Self {
        self.items.push(TrayMenuItem::action(label, id));
        self
    }

    /// Add a visual separator.
    pub fn separator(mut self) -> Self {
        self.items.push(TrayMenuItem::separator());
        self
    }

    /// Add a nested submenu.
    pub fn submenu(
        mut self,
        label: impl Into<SharedString>,
        items: impl Into<Vec<TrayMenuItem>>,
    ) -> Self {
        self.items.push(TrayMenuItem::submenu(label, items));
        self
    }

    /// Add a toggleable menu item.
    pub fn toggle(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        id: impl Into<SharedString>,
    ) -> Self {
        self.items.push(TrayMenuItem::toggle(label, checked, id));
        self
    }

    /// Return the configured context menu items.
    pub fn items(&self) -> &[TrayMenuItem] {
        &self.items
    }

    /// Count all configured items, including submenu children.
    pub fn item_count(&self) -> usize {
        TrayMenuItem::item_count(&self.items)
    }

    /// Count action items.
    pub fn action_count(&self) -> usize {
        TrayMenuItem::action_count(&self.items)
    }

    /// Count toggle items.
    pub fn toggle_count(&self) -> usize {
        TrayMenuItem::toggle_count(&self.items)
    }

    /// Count submenu items.
    pub fn submenu_count(&self) -> usize {
        TrayMenuItem::submenu_count(&self.items)
    }

    /// Count separators.
    pub fn separator_count(&self) -> usize {
        TrayMenuItem::separator_count(&self.items)
    }

    /// Count checked toggle items.
    pub fn checked_toggle_count(&self) -> usize {
        TrayMenuItem::checked_toggle_count(&self.items)
    }

    /// Content-safe summary that avoids labels and action IDs.
    pub fn to_text(&self) -> String {
        tray_menu_counts(&self.items).to_text("context menu")
    }

    /// Validate labels and action IDs before showing the menu.
    pub fn validate(&self) -> Result<()> {
        validate_tray_menu_items(&self.items)
    }

    /// Build the validated context menu item tree.
    pub fn build(self) -> Result<Vec<TrayMenuItem>> {
        self.validate()?;
        Ok(self.items)
    }

    /// Consume the builder into native menu items.
    pub fn into_items(self) -> Vec<TrayMenuItem> {
        self.items
    }
}

impl From<NativeContextMenuBuilder> for Vec<TrayMenuItem> {
    fn from(value: NativeContextMenuBuilder) -> Self {
        value.into_items()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TrayMenuCounts {
    items: usize,
    actions: usize,
    toggles: usize,
    checked_toggles: usize,
    submenus: usize,
    separators: usize,
    max_depth: usize,
}

impl TrayMenuCounts {
    fn to_text(self, label: &str) -> String {
        format!(
            "{label}: items {}, actions {}, toggles {}, checked toggles {}, submenus {}, separators {}, max depth {}",
            self.items,
            self.actions,
            self.toggles,
            self.checked_toggles,
            self.submenus,
            self.separators,
            self.max_depth
        )
    }
}

fn tray_menu_counts(items: &[TrayMenuItem]) -> TrayMenuCounts {
    let mut counts = TrayMenuCounts::default();
    let mut pending = vec![(items, 1usize)];
    while let Some((items, depth)) = pending.pop() {
        counts.max_depth = counts.max_depth.max(depth.min(MAX_NATIVE_MENU_DEPTH));
        let remaining = MAX_NATIVE_MENU_ITEMS.saturating_sub(counts.items);
        for item in items.iter().take(remaining) {
            counts.items += 1;
            match item {
                TrayMenuItem::Action { .. } => counts.actions += 1,
                TrayMenuItem::Separator => counts.separators += 1,
                TrayMenuItem::Submenu { items, .. } => {
                    counts.submenus += 1;
                    if depth < MAX_NATIVE_MENU_DEPTH {
                        pending.push((items, depth + 1));
                    }
                }
                TrayMenuItem::Toggle { checked, .. } => {
                    counts.toggles += 1;
                    if *checked {
                        counts.checked_toggles += 1;
                    }
                }
            }
        }
        if counts.items >= MAX_NATIVE_MENU_ITEMS {
            break;
        }
    }
    counts
}

const MAX_NATIVE_MENU_ITEMS: usize = 1_024;
const MAX_NATIVE_MENU_DEPTH: usize = 32;
const MAX_NATIVE_MENU_TEXT_BYTES: usize = 4_096;

fn validate_tray_menu_items(items: &[TrayMenuItem]) -> Result<()> {
    anyhow::ensure!(!items.is_empty(), "menu must contain at least one item");
    let mut action_ids = std::collections::HashSet::new();
    let mut count = 0usize;
    let mut pending = vec![(items, 1usize)];
    while let Some((items, depth)) = pending.pop() {
        anyhow::ensure!(depth <= MAX_NATIVE_MENU_DEPTH, "menu nesting is too deep");
        anyhow::ensure!(
            items.len() <= MAX_NATIVE_MENU_ITEMS.saturating_sub(count),
            "menu contains too many items"
        );
        count += items.len();
        for item in items {
            match item {
                TrayMenuItem::Action { label, id } | TrayMenuItem::Toggle { label, id, .. } => {
                    validate_menu_label(label)?;
                    validate_menu_action_id(id, &mut action_ids)?;
                }
                TrayMenuItem::Separator => {}
                TrayMenuItem::Submenu { label, items } => {
                    validate_menu_label(label)?;
                    anyhow::ensure!(!items.is_empty(), "submenu cannot be empty");
                    pending.push((items, depth + 1));
                }
            }
        }
    }
    Ok(())
}

fn validate_menu_label(label: &SharedString) -> Result<()> {
    anyhow::ensure!(
        !label.as_ref().trim().is_empty(),
        "menu label cannot be empty"
    );
    anyhow::ensure!(
        label.len() <= MAX_NATIVE_MENU_TEXT_BYTES
            && !label.chars().any(|character| character.is_control()),
        "menu label is invalid"
    );
    Ok(())
}

fn validate_menu_action_id<'a>(
    id: &'a SharedString,
    action_ids: &mut std::collections::HashSet<&'a str>,
) -> Result<()> {
    let id = id.as_ref();
    anyhow::ensure!(!id.trim().is_empty(), "menu action id cannot be empty");
    anyhow::ensure!(
        id.len() <= MAX_NATIVE_MENU_TEXT_BYTES
            && !id.chars().any(|character| character.is_control()),
        "menu action id is invalid"
    );
    anyhow::ensure!(action_ids.insert(id), "menu action id must be unique");
    Ok(())
}

#[cfg(test)]
mod tray_menu_tests {
    use super::*;

    #[test]
    fn tray_and_context_menu_summaries_are_content_safe() {
        let submenu = TrayMenuBuilder::new()
            .toggle("Secret Available", true, "presence.available")
            .action("Private Away", "presence.away")
            .build()
            .unwrap();
        let tray = TrayMenuBuilder::new()
            .action("Open Secret Project", "open.secret")
            .separator()
            .toggle("Pause Private Sync", false, "sync.pause")
            .submenu("Private Status", submenu)
            .action("Quit", "quit");

        assert_eq!(tray.item_count(), 7);
        assert_eq!(tray.action_count(), 3);
        assert_eq!(tray.toggle_count(), 2);
        assert_eq!(tray.checked_toggle_count(), 1);
        assert_eq!(tray.submenu_count(), 1);
        assert_eq!(tray.separator_count(), 1);
        assert!(tray.validate().is_ok());

        let summary = tray.to_text();
        assert_eq!(
            summary,
            "tray menu: items 7, actions 3, toggles 2, checked toggles 1, submenus 1, separators 1, max depth 2"
        );
        assert!(!summary.contains("Secret"));
        assert!(!summary.contains("open.secret"));
        assert!(!summary.contains("presence"));
        assert!(!summary.contains("Private"));

        let built = tray.build().unwrap();
        let item_summary = TrayMenuItem::items_to_text(&built);
        assert!(item_summary.contains("menu items: items 7"));
        assert_eq!(TrayMenuItem::item_count(&built), 7);
        assert_eq!(TrayMenuItem::checked_toggle_count(&built), 1);
        assert!(!item_summary.contains("Secret"));
        assert!(!item_summary.contains("open.secret"));

        let context = NativeContextMenuBuilder::new()
            .action("Copy Private Path", "copy.path")
            .separator()
            .toggle("Show Hidden Files", true, "show.hidden");
        assert_eq!(
            context.to_text(),
            "context menu: items 3, actions 1, toggles 1, checked toggles 1, submenus 0, separators 1, max depth 1"
        );
        assert!(!context.to_text().contains("Private"));
        assert!(!context.to_text().contains("copy.path"));
    }

    #[test]
    fn native_menu_validation_is_bounded_iterative_and_content_safe() {
        let duplicate = vec![
            TrayMenuItem::action("Private first", "secret.identifier"),
            TrayMenuItem::action("Private second", "secret.identifier"),
        ];
        let error = TrayMenuItem::validate_items(&duplicate)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("secret.identifier"));
        assert!(!error.contains("Private"));

        let oversized = vec![TrayMenuItem::separator(); MAX_NATIVE_MENU_ITEMS + 1];
        assert!(TrayMenuItem::validate_items(&oversized).is_err());
        assert_eq!(TrayMenuItem::item_count(&oversized), MAX_NATIVE_MENU_ITEMS);

        let mut nested = TrayMenuItem::action("Leaf", "leaf");
        for depth in 0..MAX_NATIVE_MENU_DEPTH {
            nested = TrayMenuItem::submenu(format!("Level {depth}"), vec![nested]);
        }
        assert!(TrayMenuItem::validate_items(&[nested]).is_err());
    }
}

/// A platform shell target that can be opened or revealed by the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellTarget {
    /// Open a URL with the system default browser or registered URL handler.
    Url(String),
    /// Open a file or directory with the system default application.
    Path(PathBuf),
    /// Reveal a file or directory in the platform file manager.
    RevealPath(PathBuf),
}

impl ShellTarget {
    /// Create a URL shell target.
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// Create a path shell target.
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    /// Create a reveal-in-folder shell target.
    pub fn reveal_path(path: impl Into<PathBuf>) -> Self {
        Self::RevealPath(path.into())
    }

    /// Validate this shell target before dispatching it to the OS shell.
    pub fn validate(&self) -> Result<()> {
        match self {
            ShellTarget::Url(url) => validate_shell_url(url),
            ShellTarget::Path(path) | ShellTarget::RevealPath(path) => {
                validate_shell_path(path, false)
            }
        }
    }
}

/// Checked, inspectable plan for ordered shell open/reveal workflows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellTargetsPlan {
    targets: Vec<ShellTarget>,
    require_existing_paths: bool,
    canonicalized_paths: bool,
}

impl ShellTargetsPlan {
    /// Shell targets in dispatch order.
    pub fn targets(&self) -> &[ShellTarget] {
        &self.targets
    }

    /// Consume the plan and return the ordered shell targets.
    pub fn into_targets(self) -> Vec<ShellTarget> {
        self.targets
    }

    /// Number of shell targets in the plan.
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// Whether this plan contains no shell targets.
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Whether path and reveal targets were required to exist.
    pub fn requires_existing_paths(&self) -> bool {
        self.require_existing_paths
    }

    /// Whether path and reveal targets were canonicalized while building.
    pub fn canonicalized_paths(&self) -> bool {
        self.canonicalized_paths
    }

    /// Whether this plan includes URL targets.
    pub fn contains_url_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, ShellTarget::Url(_)))
    }

    /// Whether this plan includes file or directory open targets.
    pub fn contains_path_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, ShellTarget::Path(_)))
    }

    /// Whether this plan includes reveal-in-folder targets.
    pub fn contains_reveal_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, ShellTarget::RevealPath(_)))
    }

    /// Whether dispatching this plan requires URL-open capability.
    pub fn requires_open_external_url(&self) -> bool {
        self.contains_url_targets()
    }

    /// Whether dispatching this plan requires shell-execute capability.
    pub fn requires_shell_execute(&self) -> bool {
        self.contains_path_targets() || self.contains_reveal_targets()
    }

    /// Content-safe summary for shell open/reveal traces.
    pub fn to_text(&self) -> String {
        format!(
            "shell targets: {} targets, url {}, path {}, reveal {}, existing {}, canonicalized {}",
            self.len(),
            self.contains_url_targets(),
            self.contains_path_targets(),
            self.contains_reveal_targets(),
            self.requires_existing_paths(),
            self.canonicalized_paths()
        )
    }
}

/// Builder for opening or revealing multiple platform shell targets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellTargetsBuilder {
    targets: Vec<ShellTarget>,
    require_existing_paths: bool,
    canonicalize_paths: bool,
}

impl ShellTargetsBuilder {
    /// Create an empty shell-target builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one typed shell target.
    pub fn target(mut self, target: ShellTarget) -> Self {
        self.targets.push(target);
        self
    }

    /// Add a URL target.
    pub fn url(self, url: impl Into<String>) -> Self {
        self.target(ShellTarget::url(url))
    }

    /// Add a file or directory target.
    pub fn path(self, path: impl Into<PathBuf>) -> Self {
        self.target(ShellTarget::path(path))
    }

    /// Add a reveal-in-folder target.
    pub fn reveal_path(self, path: impl Into<PathBuf>) -> Self {
        self.target(ShellTarget::reveal_path(path))
    }

    /// Add multiple typed shell targets.
    pub fn targets(mut self, targets: impl IntoIterator<Item = ShellTarget>) -> Self {
        self.targets.extend(targets);
        self
    }

    /// Require path and reveal targets to exist before building.
    pub fn require_existing_paths(mut self) -> Self {
        self.require_existing_paths = true;
        self
    }

    /// Canonicalize path and reveal targets before dispatching them.
    pub fn canonicalize_paths(mut self) -> Self {
        self.canonicalize_paths = true;
        self.require_existing_paths = true;
        self
    }

    /// Return the configured shell targets.
    pub fn configured_targets(&self) -> &[ShellTarget] {
        &self.targets
    }

    /// Number of configured shell targets.
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// Whether no shell targets are configured yet.
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Whether this builder includes URL targets.
    pub fn contains_url_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, ShellTarget::Url(_)))
    }

    /// Whether this builder includes file or directory open targets.
    pub fn contains_path_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, ShellTarget::Path(_)))
    }

    /// Whether this builder includes reveal-in-folder targets.
    pub fn contains_reveal_targets(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target, ShellTarget::RevealPath(_)))
    }

    /// Whether path and reveal targets will be required to exist.
    pub fn requires_existing_paths(&self) -> bool {
        self.require_existing_paths
    }

    /// Whether path and reveal targets will be canonicalized while building.
    pub fn canonicalizes_paths(&self) -> bool {
        self.canonicalize_paths
    }

    /// Content-safe summary for generated shell-target traces.
    pub fn to_text(&self) -> String {
        format!(
            "shell targets builder: {} targets, url {}, path {}, reveal {}, existing {}, canonicalize {}",
            self.len(),
            self.contains_url_targets(),
            self.contains_path_targets(),
            self.contains_reveal_targets(),
            self.requires_existing_paths(),
            self.canonicalizes_paths()
        )
    }

    /// Validate that at least one shell target was configured.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.targets.is_empty(),
            "at least one shell target must be configured"
        );
        for target in &self.targets {
            validate_shell_target(target, self.require_existing_paths)?;
        }
        Ok(())
    }

    /// Build the validated shell-target list.
    pub fn build(self) -> Result<Vec<ShellTarget>> {
        Ok(self.build_checked()?.into_targets())
    }

    /// Build a checked, inspectable shell-target plan.
    pub fn build_checked(mut self) -> Result<ShellTargetsPlan> {
        self.validate()?;
        let canonicalized_paths = self.canonicalize_paths;
        if self.canonicalize_paths {
            for target in &mut self.targets {
                match target {
                    ShellTarget::Path(path) | ShellTarget::RevealPath(path) => {
                        *path = path.canonicalize().map_err(|error| {
                            anyhow::anyhow!(
                                "could not canonicalize shell target path {}: {error}",
                                path.display()
                            )
                        })?;
                    }
                    ShellTarget::Url(_) => {}
                }
            }
        }
        Ok(ShellTargetsPlan {
            targets: self.targets,
            require_existing_paths: self.require_existing_paths,
            canonicalized_paths,
        })
    }
}

impl From<ShellTarget> for ShellTargetsBuilder {
    fn from(value: ShellTarget) -> Self {
        Self::new().target(value)
    }
}

/// Checked request to move a file or directory to the platform trash/recycle bin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRequest {
    path: PathBuf,
    require_existing_path: bool,
    canonicalized: bool,
    allow_relative_path: bool,
}

impl TrashRequest {
    /// Create a builder for a trash request.
    pub fn builder(path: impl Into<PathBuf>) -> TrashRequestBuilder {
        TrashRequestBuilder::new(path)
    }

    /// Path requested for trash/recycle.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the request required the path to exist.
    pub fn requires_existing_path(&self) -> bool {
        self.require_existing_path
    }

    /// Whether the built path was canonicalized.
    pub fn is_canonicalized(&self) -> bool {
        self.canonicalized
    }

    /// Whether the request explicitly allowed a relative path.
    pub fn allows_relative_path(&self) -> bool {
        self.allow_relative_path
    }

    /// Best-effort display name for prompts, logs, and generated confirmations.
    pub fn display_name(&self) -> Option<&str> {
        self.path.file_name().and_then(|name| name.to_str())
    }

    /// Parent directory of the target, when available.
    pub fn parent_path(&self) -> Option<&Path> {
        self.path.parent()
    }

    /// Whether dispatching this request requires shell-execute/trash capability.
    pub fn requires_shell_execute(&self) -> bool {
        true
    }

    /// Stable diagnostic text for generated trash/recycle flows.
    pub fn to_text(&self) -> String {
        format!(
            "trash request: display-name {}, parent {}, require existing path {}, canonicalized {}, allow relative path {}",
            self.display_name().is_some(),
            self.parent_path().is_some(),
            self.require_existing_path,
            self.canonicalized,
            self.allow_relative_path
        )
    }
}

/// Builder for checked platform trash/recycle requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRequestBuilder {
    path: PathBuf,
    require_existing_path: bool,
    canonicalize_path: bool,
    allow_relative_path: bool,
}

impl TrashRequestBuilder {
    /// Create a trash request for a file or directory path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            require_existing_path: true,
            canonicalize_path: false,
            allow_relative_path: false,
        }
    }

    /// Require the path to exist before trashing.
    pub fn require_existing_path(mut self) -> Self {
        self.require_existing_path = true;
        self
    }

    /// Canonicalize the path before trashing.
    pub fn canonicalize_path(mut self) -> Self {
        self.canonicalize_path = true;
        self.require_existing_path = true;
        self
    }

    /// Allow relative paths for app-owned sandbox directories.
    pub fn allow_relative_path(mut self) -> Self {
        self.allow_relative_path = true;
        self
    }

    /// Whether the builder requires the path to exist.
    pub fn requires_existing_path(&self) -> bool {
        self.require_existing_path
    }

    /// Whether the builder will canonicalize the path.
    pub fn canonicalizes_path(&self) -> bool {
        self.canonicalize_path
    }

    /// Whether the builder allows a relative path.
    pub fn allows_relative_path(&self) -> bool {
        self.allow_relative_path
    }

    /// Whether the configured path appears relative.
    pub fn has_relative_path(&self) -> bool {
        self.path.is_relative()
    }

    /// Content-safe summary for generated trash/recycle request traces.
    pub fn to_text(&self) -> String {
        format!(
            "trash request builder: relative {}, require existing path {}, canonicalize {}, allow relative path {}",
            self.has_relative_path(),
            self.requires_existing_path(),
            self.canonicalizes_path(),
            self.allows_relative_path()
        )
    }

    /// Validate this trash request without mutating the filesystem.
    pub fn validate(&self) -> Result<()> {
        validate_trash_path(
            &self.path,
            self.require_existing_path,
            self.allow_relative_path,
        )
    }

    /// Build a checked trash request.
    pub fn build_checked(mut self) -> Result<TrashRequest> {
        self.validate()?;
        let mut canonicalized = false;
        if self.canonicalize_path {
            self.path = self.path.canonicalize().map_err(|error| {
                anyhow::anyhow!(
                    "could not canonicalize trash request path {}: {error}",
                    self.path.display()
                )
            })?;
            canonicalized = true;
        }
        Ok(TrashRequest {
            path: self.path,
            require_existing_path: self.require_existing_path,
            canonicalized,
            allow_relative_path: self.allow_relative_path,
        })
    }
}

fn validate_shell_target(target: &ShellTarget, require_existing_paths: bool) -> Result<()> {
    match target {
        ShellTarget::Url(url) => validate_shell_url(url),
        ShellTarget::Path(path) | ShellTarget::RevealPath(path) => {
            validate_shell_path(path, require_existing_paths)
        }
    }
}

fn validate_shell_url(url: &str) -> Result<()> {
    anyhow::ensure!(!url.trim().is_empty(), "shell URL cannot be empty");
    anyhow::ensure!(
        url == url.trim(),
        "shell URL cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !url.chars().any(char::is_control),
        "shell URL cannot contain control characters"
    );
    let parsed = http_client::Url::parse(url)
        .map_err(|error| anyhow::anyhow!("shell URL is invalid: {error}"))?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https" | "mailto"),
        "shell URL must use http, https, or mailto"
    );
    if matches!(parsed.scheme(), "http" | "https") {
        anyhow::ensure!(parsed.host_str().is_some(), "shell URL must include a host");
    }
    Ok(())
}

fn validate_shell_path(path: &Path, require_existing_path: bool) -> Result<()> {
    anyhow::ensure!(!path.as_os_str().is_empty(), "shell path cannot be empty");
    if let Some(text) = path.to_str() {
        anyhow::ensure!(!text.contains('\0'), "shell path cannot contain NUL bytes");
    }
    if require_existing_path {
        std::fs::metadata(path).map_err(|error| {
            anyhow::anyhow!("shell path does not exist {}: {error}", path.display())
        })?;
    }
    Ok(())
}

fn validate_trash_path(
    path: &Path,
    require_existing_path: bool,
    allow_relative_path: bool,
) -> Result<()> {
    validate_shell_path(path, false)?;
    anyhow::ensure!(
        allow_relative_path || path.is_absolute(),
        "trash request path must be absolute unless relative paths are explicitly allowed"
    );
    anyhow::ensure!(
        path.file_name().is_some(),
        "trash request path must not target a filesystem root"
    );
    if require_existing_path {
        std::fs::metadata(path).map_err(|error| {
            anyhow::anyhow!(
                "trash request path does not exist {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// A global hotkey registration with an application-owned identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalHotkey {
    id: u32,
    keystroke: Keystroke,
    name: Option<SharedString>,
}

impl GlobalHotkey {
    /// Create a global hotkey from a parsed keystroke.
    pub fn new(id: u32, keystroke: Keystroke) -> Self {
        Self {
            id,
            keystroke,
            name: None,
        }
    }

    /// Create a global hotkey by parsing a keystroke string.
    pub fn parse(id: u32, keystroke: &str) -> std::result::Result<Self, InvalidKeystrokeError> {
        Ok(Self::new(id, Keystroke::parse(keystroke)?))
    }

    /// Attach a human-readable name for logs, settings, or callbacks.
    pub fn named(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The application-owned hotkey identifier.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The parsed keystroke registered with the platform.
    pub fn keystroke(&self) -> &Keystroke {
        &self.keystroke
    }

    /// Optional human-readable name.
    pub fn name(&self) -> Option<&SharedString> {
        self.name.as_ref()
    }

    /// Whether this hotkey has a human-readable name.
    pub fn has_name(&self) -> bool {
        self.name.is_some()
    }

    /// Return a compact summary without exposing shortcut text or names.
    pub fn to_text(&self) -> String {
        format!(
            "global hotkey: id {}, name {}, shortcut true",
            self.id(),
            self.has_name()
        )
    }
}

/// A collection of global hotkeys ready to register.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalHotkeySet {
    hotkeys: Vec<GlobalHotkey>,
}

impl GlobalHotkeySet {
    /// Create an empty global hotkey set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a parsed global hotkey.
    pub fn hotkey(mut self, hotkey: GlobalHotkey) -> Self {
        self.hotkeys.push(hotkey);
        self
    }

    /// Return registered hotkeys.
    pub fn hotkeys(&self) -> &[GlobalHotkey] {
        &self.hotkeys
    }

    /// Number of configured global hotkeys.
    pub fn len(&self) -> usize {
        self.hotkeys.len()
    }

    /// Whether this set contains no global hotkeys.
    pub fn is_empty(&self) -> bool {
        self.hotkeys.is_empty()
    }

    /// Application-owned hotkey ids in registration order.
    pub fn ids(&self) -> Vec<u32> {
        self.hotkeys.iter().map(GlobalHotkey::id).collect()
    }

    /// Parsed keystrokes in registration order.
    pub fn keystrokes(&self) -> Vec<&Keystroke> {
        self.hotkeys.iter().map(GlobalHotkey::keystroke).collect()
    }

    /// Human-readable names in registration order.
    pub fn names(&self) -> Vec<Option<&str>> {
        self.hotkeys
            .iter()
            .map(|hotkey| hotkey.name().map(|name| name.as_ref()))
            .collect()
    }

    /// Number of hotkeys with human-readable names.
    pub fn named_count(&self) -> usize {
        self.hotkeys
            .iter()
            .filter(|hotkey| hotkey.name().is_some())
            .count()
    }

    /// Number of hotkeys without human-readable names.
    pub fn unnamed_count(&self) -> usize {
        self.len().saturating_sub(self.named_count())
    }

    /// Whether this set includes a specific application-owned id.
    pub fn contains_id(&self, id: u32) -> bool {
        self.hotkeys.iter().any(|hotkey| hotkey.id() == id)
    }

    /// Returns a compact summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "global hotkey set: {} hotkeys, {} named, {} unnamed, ids [{}]",
            self.len(),
            self.named_count(),
            self.unnamed_count(),
            self.ids()
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Validate the set before registering it with the platform.
    pub fn validate(&self) -> Result<()> {
        validate_global_hotkeys(&self.hotkeys)
    }

    /// Consume the set into its hotkeys.
    pub fn into_hotkeys(self) -> Vec<GlobalHotkey> {
        self.hotkeys
    }
}

impl From<GlobalHotkey> for GlobalHotkeySet {
    fn from(value: GlobalHotkey) -> Self {
        Self::new().hotkey(value)
    }
}

impl From<GlobalHotkeyBuilder> for GlobalHotkeySet {
    fn from(value: GlobalHotkeyBuilder) -> Self {
        value.build()
    }
}

/// Checked global hotkey unregistration request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalHotkeyUnregistration {
    ids: Vec<u32>,
}

impl GlobalHotkeyUnregistration {
    /// Create an empty unregistration request.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a global hotkey id to unregister.
    pub fn id(mut self, id: u32) -> Self {
        self.ids.push(id);
        self
    }

    /// Add all ids from a registered hotkey set.
    pub fn hotkey_set(mut self, hotkeys: &GlobalHotkeySet) -> Self {
        self.ids
            .extend(hotkeys.hotkeys().iter().map(GlobalHotkey::id));
        self
    }

    /// Return the configured ids.
    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    /// Number of ids configured for unregistration.
    pub fn id_count(&self) -> usize {
        self.ids.len()
    }

    /// Returns a compact summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "global hotkey unregistration: {} ids [{}]",
            self.id_count(),
            self.ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Validate the request before unregistering platform hotkeys.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.ids.is_empty(),
            "at least one global hotkey id must be configured for unregistration"
        );

        let mut ids = std::collections::HashSet::new();
        for id in &self.ids {
            anyhow::ensure!(
                ids.insert(*id),
                "global hotkey unregister id must be unique: {id}"
            );
        }

        Ok(())
    }
}

/// Builder for global hotkey registrations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalHotkeyBuilder {
    hotkeys: Vec<GlobalHotkey>,
}

impl GlobalHotkeyBuilder {
    /// Create an empty global hotkey builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a global hotkey from a parsed keystroke.
    pub fn hotkey(mut self, id: u32, keystroke: Keystroke) -> Self {
        self.hotkeys.push(GlobalHotkey::new(id, keystroke));
        self
    }

    /// Add a named global hotkey from a parsed keystroke.
    pub fn named_hotkey(
        mut self,
        id: u32,
        name: impl Into<SharedString>,
        keystroke: Keystroke,
    ) -> Self {
        self.hotkeys
            .push(GlobalHotkey::new(id, keystroke).named(name));
        self
    }

    /// Add a global hotkey by parsing a keystroke string.
    pub fn parse_hotkey(
        mut self,
        id: u32,
        keystroke: &str,
    ) -> std::result::Result<Self, InvalidKeystrokeError> {
        self.hotkeys.push(GlobalHotkey::parse(id, keystroke)?);
        Ok(self)
    }

    /// Add a named global hotkey by parsing a keystroke string.
    pub fn parse_named_hotkey(
        mut self,
        id: u32,
        name: impl Into<SharedString>,
        keystroke: &str,
    ) -> std::result::Result<Self, InvalidKeystrokeError> {
        self.hotkeys
            .push(GlobalHotkey::parse(id, keystroke)?.named(name));
        Ok(self)
    }

    /// Return configured hotkeys.
    pub fn hotkeys(&self) -> &[GlobalHotkey] {
        &self.hotkeys
    }

    /// Number of configured global hotkeys.
    pub fn len(&self) -> usize {
        self.hotkeys.len()
    }

    /// Whether no global hotkeys are configured yet.
    pub fn is_empty(&self) -> bool {
        self.hotkeys.is_empty()
    }

    /// Application-owned hotkey ids in configured order.
    pub fn ids(&self) -> Vec<u32> {
        self.hotkeys.iter().map(GlobalHotkey::id).collect()
    }

    /// Number of hotkeys with human-readable names.
    pub fn named_count(&self) -> usize {
        self.hotkeys
            .iter()
            .filter(|hotkey| hotkey.name().is_some())
            .count()
    }

    /// Number of hotkeys without human-readable names.
    pub fn unnamed_count(&self) -> usize {
        self.len().saturating_sub(self.named_count())
    }

    /// Whether this builder includes a specific application-owned id.
    pub fn contains_id(&self, id: u32) -> bool {
        self.hotkeys.iter().any(|hotkey| hotkey.id() == id)
    }

    /// Return a compact summary before registering platform hotkeys.
    pub fn to_text(&self) -> String {
        format!(
            "global hotkey builder: {} hotkeys, {} named, {} unnamed, ids [{}]",
            self.len(),
            self.named_count(),
            self.unnamed_count(),
            self.ids()
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Validate the configured hotkeys.
    pub fn validate(&self) -> Result<()> {
        validate_global_hotkeys(&self.hotkeys)
    }

    /// Build a validated hotkey set.
    pub fn build_checked(self) -> Result<GlobalHotkeySet> {
        self.validate()?;
        Ok(self.build())
    }

    /// Build the hotkey set.
    pub fn build(self) -> GlobalHotkeySet {
        GlobalHotkeySet {
            hotkeys: self.hotkeys,
        }
    }
}

fn validate_global_hotkeys(hotkeys: &[GlobalHotkey]) -> Result<()> {
    anyhow::ensure!(
        !hotkeys.is_empty(),
        "at least one global hotkey must be configured"
    );

    let mut ids = std::collections::HashSet::new();
    let mut keystrokes = std::collections::HashSet::new();
    for hotkey in hotkeys {
        if let Some(name) = hotkey.name() {
            validate_global_hotkey_name(name.as_ref())?;
        }
        anyhow::ensure!(
            ids.insert(hotkey.id()),
            "global hotkey id must be unique: {}",
            hotkey.id()
        );
        anyhow::ensure!(
            keystrokes.insert(hotkey.keystroke()),
            "global hotkey keystroke must be unique: {}",
            hotkey.keystroke()
        );
    }

    Ok(())
}

fn validate_global_hotkey_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.trim().is_empty(),
        "global hotkey name cannot be empty"
    );
    anyhow::ensure!(
        name.trim() == name,
        "global hotkey name cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        name.len() <= 128,
        "global hotkey name cannot exceed 128 bytes"
    );
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "global hotkey name cannot contain control characters"
    );
    Ok(())
}

/// Information about the currently focused window from any application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedWindowInfo {
    /// The name of the application that owns the focused window.
    pub app_name: String,
    /// The title of the focused window.
    pub window_title: String,
    /// The bundle identifier of the application (macOS only).
    pub bundle_id: Option<String>,
    /// The process ID of the application.
    pub pid: Option<u32>,
}

impl FocusedWindowInfo {
    /// Returns true when this focused window belongs to the current process.
    pub fn is_current_process(&self) -> bool {
        self.pid == Some(std::process::id())
    }

    /// Returns true when this focused window belongs to another process.
    pub fn is_external_process(&self) -> bool {
        self.pid.is_some_and(|pid| pid != std::process::id())
    }

    /// Whether the platform supplied a non-empty window title.
    pub fn has_title(&self) -> bool {
        !self.window_title.trim().is_empty()
    }

    /// Whether the platform supplied a bundle identifier.
    pub fn has_bundle_id(&self) -> bool {
        self.bundle_id.is_some()
    }

    /// Whether the platform supplied a process id.
    pub fn has_pid(&self) -> bool {
        self.pid.is_some()
    }

    /// Content-safe summary for active-window diagnostics and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "focused window: title {}, bundle {}, pid {}, external {}",
            self.has_title(),
            self.has_bundle_id(),
            self.has_pid(),
            self.is_external_process()
        )
    }

    /// Validate metadata shape supplied by a platform backend or test fixture.
    pub fn validate(&self) -> Result<()> {
        validate_focused_window_text("focused window app name", &self.app_name, true)?;
        validate_focused_window_text("focused window title", &self.window_title, false)?;
        if let Some(bundle_id) = &self.bundle_id {
            validate_focused_window_text("focused window bundle id", bundle_id, true)?;
        }
        anyhow::ensure!(self.pid != Some(0), "focused window pid cannot be zero");
        Ok(())
    }

    /// Return whether this focused-window record satisfies a checked query.
    pub fn matches_query(&self, query: &FocusedWindowQuery) -> bool {
        if self.validate().is_err() {
            return false;
        }
        if query.require_title && self.window_title.trim().is_empty() {
            return false;
        }
        if query.require_pid && self.pid.is_none() {
            return false;
        }
        if query.external_only && !self.is_external_process() {
            return false;
        }
        if query.current_process_only && !self.is_current_process() {
            return false;
        }
        if let Some(pid) = query.pid
            && self.pid != Some(pid)
        {
            return false;
        }
        if let Some(app_name) = &query.app_name
            && !self.app_name.eq_ignore_ascii_case(app_name)
        {
            return false;
        }
        if let Some(contains) = &query.app_name_contains
            && !self
                .app_name
                .to_ascii_lowercase()
                .contains(&contains.to_ascii_lowercase())
        {
            return false;
        }
        if let Some(bundle_id) = &query.bundle_id
            && self.bundle_id.as_deref() != Some(bundle_id.as_str())
        {
            return false;
        }
        true
    }
}

/// A checked filter for querying the currently focused external window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedWindowQuery {
    require_title: bool,
    require_pid: bool,
    external_only: bool,
    current_process_only: bool,
    app_name: Option<String>,
    app_name_contains: Option<String>,
    bundle_id: Option<String>,
    pid: Option<u32>,
}

impl FocusedWindowQuery {
    /// Create a focused-window query builder.
    pub fn builder() -> FocusedWindowQueryBuilder {
        FocusedWindowQueryBuilder::new()
    }

    /// Return whether a focused window title is required.
    pub fn requires_title(&self) -> bool {
        self.require_title
    }

    /// Return whether a process id is required.
    pub fn requires_pid(&self) -> bool {
        self.require_pid
    }

    /// Return whether only windows outside the current process should match.
    pub fn external_only(&self) -> bool {
        self.external_only
    }

    /// Return whether only windows in the current process should match.
    pub fn current_process_only(&self) -> bool {
        self.current_process_only
    }

    /// Return the exact app-name filter, if any.
    pub fn app_name(&self) -> Option<&str> {
        self.app_name.as_deref()
    }

    /// Return the app-name substring filter, if any.
    pub fn app_name_contains(&self) -> Option<&str> {
        self.app_name_contains.as_deref()
    }

    /// Return the exact bundle-id filter, if any.
    pub fn bundle_id(&self) -> Option<&str> {
        self.bundle_id.as_deref()
    }

    /// Return the exact process-id filter, if any.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Whether the query carries any app, bundle, or pid filter.
    pub fn has_filter(&self) -> bool {
        self.app_name.is_some()
            || self.app_name_contains.is_some()
            || self.bundle_id.is_some()
            || self.pid.is_some()
    }

    /// Whether the query constrains the process relationship.
    pub fn has_process_scope(&self) -> bool {
        self.external_only || self.current_process_only
    }

    /// Content-safe summary for checked focused-window queries.
    pub fn to_text(&self) -> String {
        format!(
            "focused window query: title {}, pid {}, scope {}, filters {}",
            self.require_title,
            self.require_pid,
            self.has_process_scope(),
            self.has_filter()
        )
    }

    /// Validate the query before reading platform state.
    pub fn validate(&self) -> Result<()> {
        if let Some(app_name) = &self.app_name {
            validate_focused_window_text("focused window app filter", app_name, true)?;
        }
        if let Some(app_name_contains) = &self.app_name_contains {
            validate_focused_window_text(
                "focused window app contains filter",
                app_name_contains,
                true,
            )?;
        }
        if let Some(bundle_id) = &self.bundle_id {
            validate_focused_window_text("focused window bundle id filter", bundle_id, true)?;
        }
        anyhow::ensure!(
            !(self.external_only && self.current_process_only),
            "focused window query cannot require both external and current process"
        );
        anyhow::ensure!(
            !(self.app_name.is_some() && self.app_name_contains.is_some()),
            "focused window query cannot use exact and contains app-name filters together"
        );
        anyhow::ensure!(
            self.pid != Some(0),
            "focused window pid filter cannot be zero"
        );
        Ok(())
    }
}

/// Builder for checked focused-window queries.
#[derive(Debug, Clone, Default)]
pub struct FocusedWindowQueryBuilder {
    query: FocusedWindowQuery,
}

impl FocusedWindowQueryBuilder {
    /// Create an empty query that accepts any valid focused-window record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Require a non-empty window title.
    pub fn require_title(mut self) -> Self {
        self.query.require_title = true;
        self
    }

    /// Require process id metadata from the platform backend.
    pub fn require_pid(mut self) -> Self {
        self.query.require_pid = true;
        self
    }

    /// Match only windows owned by another process.
    pub fn external_only(mut self) -> Self {
        self.query.external_only = true;
        self
    }

    /// Match only windows owned by the current process.
    pub fn current_process_only(mut self) -> Self {
        self.query.current_process_only = true;
        self
    }

    /// Match an exact application name.
    pub fn app_name(mut self, app_name: impl Into<String>) -> Self {
        self.query.app_name = Some(app_name.into());
        self
    }

    /// Match application names containing the given text, case-insensitively.
    pub fn app_name_contains(mut self, app_name: impl Into<String>) -> Self {
        self.query.app_name_contains = Some(app_name.into());
        self
    }

    /// Match an exact bundle identifier. This is primarily useful on macOS.
    pub fn bundle_id(mut self, bundle_id: impl Into<String>) -> Self {
        self.query.bundle_id = Some(bundle_id.into());
        self
    }

    /// Match an exact process id.
    pub fn pid(mut self, pid: u32) -> Self {
        self.query.pid = Some(pid);
        self
    }

    /// Validate the configured query.
    pub fn validate(&self) -> Result<()> {
        self.query.validate()
    }

    /// Content-safe summary for generated focused-window query traces.
    pub fn to_text(&self) -> String {
        self.query.to_text()
    }

    /// Build the checked query.
    pub fn build_checked(self) -> Result<FocusedWindowQuery> {
        self.query.validate()?;
        Ok(self.query)
    }
}

impl From<FocusedWindowQuery> for FocusedWindowQueryBuilder {
    fn from(query: FocusedWindowQuery) -> Self {
        Self { query }
    }
}

fn validate_focused_window_text(label: &str, value: &str, require_non_empty: bool) -> Result<()> {
    if require_non_empty {
        anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    }
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

/// The status of a system permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PermissionStatus {
    /// Permission has been granted.
    Granted,
    /// Permission has been denied.
    Denied,
    /// Permission has not yet been requested.
    NotDetermined,
    /// Permission is restricted by system policy (e.g. parental controls).
    Restricted,
}

/// System power state change events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPowerEvent {
    /// The system is about to suspend/sleep.
    Suspend,
    /// The system has resumed from suspend/sleep.
    Resume,
    /// The system power policy changed, such as entering or leaving battery saver mode.
    PowerModeChanged,
    /// The screen has been locked.
    LockScreen,
    /// The screen has been unlocked.
    UnlockScreen,
    /// The system is shutting down.
    Shutdown,
}

impl SystemPowerEvent {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Suspend => "suspend",
            Self::Resume => "resume",
            Self::PowerModeChanged => "power mode changed",
            Self::LockScreen => "lock screen",
            Self::UnlockScreen => "unlock screen",
            Self::Shutdown => "shutdown",
        }
    }
}

/// The kind of power save blocker to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSaveBlockerKind {
    /// Prevent the application from being suspended.
    PreventAppSuspension,
    /// Prevent the display from sleeping.
    PreventDisplaySleep,
}

impl PowerSaveBlockerKind {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::PreventAppSuspension => "prevent app suspension",
            Self::PreventDisplaySleep => "prevent display sleep",
        }
    }
}

/// The system's current power policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerMode {
    /// Full performance mode, typically when external power is available.
    #[default]
    Performance,
    /// Reduced-power mode, typically when the system is running on battery.
    Balanced,
    /// The system's low-power or battery-saver mode is active.
    LowPower,
}

impl PowerMode {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Balanced => "balanced",
            Self::LowPower => "low power",
        }
    }
}

/// The system's current external-power or battery source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemPowerSource {
    /// The platform does not expose the current power source.
    #[default]
    Unknown,
    /// The system is connected to external/AC power and is not reporting charging state.
    ExternalPower,
    /// The system is running on battery and is not reporting charging state.
    Battery,
    /// The battery is currently charging from external power.
    Charging,
    /// The battery is currently discharging.
    Discharging,
}

impl SystemPowerSource {
    /// Whether the source means the app should assume battery-powered operation.
    pub fn is_on_battery(self) -> bool {
        matches!(self, Self::Battery | Self::Discharging)
    }

    /// Whether the source means the app should assume external power is available.
    pub fn is_external_power(self) -> bool {
        matches!(self, Self::ExternalPower | Self::Charging)
    }

    /// Whether the platform reported a concrete power source.
    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::ExternalPower => "external power",
            Self::Battery => "battery",
            Self::Charging => "charging",
            Self::Discharging => "discharging",
        }
    }
}

/// The current network connectivity status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkStatus {
    /// The system has network connectivity.
    Online,
    /// The system has no network connectivity.
    Offline,
}

/// Media key events from hardware media keys or OS media controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKeyEvent {
    /// Play media.
    Play,
    /// Pause media.
    Pause,
    /// Toggle play/pause.
    PlayPause,
    /// Stop media playback.
    Stop,
    /// Skip to the next track.
    NextTrack,
    /// Skip to the previous track.
    PreviousTrack,
}

/// The type of user attention to request from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionType {
    /// An informational attention request (e.g. bounce dock icon once).
    Informational,
    /// A critical attention request (e.g. bounce dock icon continuously).
    Critical,
}

/// The state of a taskbar/dock progress bar for a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressBarState {
    /// No progress bar is shown.
    None,
    /// An indeterminate progress bar is shown.
    Indeterminate,
    /// A normal progress bar with the given fraction (0.0 to 1.0).
    Normal(f64),
    /// An error progress bar with the given fraction (0.0 to 1.0).
    Error(f64),
    /// A paused progress bar with the given fraction (0.0 to 1.0).
    Paused(f64),
}

impl ProgressBarState {
    /// Create a normal progress bar state after validating the fraction.
    pub fn normal(fraction: f64) -> Result<Self> {
        validate_progress_fraction(fraction)?;
        Ok(Self::Normal(fraction))
    }

    /// Create an error progress bar state after validating the fraction.
    pub fn error(fraction: f64) -> Result<Self> {
        validate_progress_fraction(fraction)?;
        Ok(Self::Error(fraction))
    }

    /// Create a paused progress bar state after validating the fraction.
    pub fn paused(fraction: f64) -> Result<Self> {
        validate_progress_fraction(fraction)?;
        Ok(Self::Paused(fraction))
    }

    /// Validate this progress state before passing it to the platform backend.
    pub fn validate(&self) -> Result<()> {
        match *self {
            Self::None | Self::Indeterminate => Ok(()),
            Self::Normal(fraction) | Self::Error(fraction) | Self::Paused(fraction) => {
                validate_progress_fraction(fraction)
            }
        }
    }

    /// Stable state name for diagnostics and generated UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Indeterminate => "indeterminate",
            Self::Normal(_) => "normal",
            Self::Error(_) => "error",
            Self::Paused(_) => "paused",
        }
    }

    /// Whether this progress state carries a validated fraction.
    pub fn is_determinate(&self) -> bool {
        matches!(self, Self::Normal(_) | Self::Error(_) | Self::Paused(_))
    }

    /// Whether this progress state clears the platform indicator.
    pub fn is_clear(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Content-safe summary for taskbar/dock progress traces.
    pub fn to_text(&self) -> String {
        format!(
            "window progress: kind {}, determinate {}",
            self.kind(),
            self.is_determinate()
        )
    }
}

fn validate_progress_fraction(fraction: f64) -> Result<()> {
    anyhow::ensure!(
        fraction.is_finite() && (0.0..=1.0).contains(&fraction),
        "progress fraction must be finite and between 0.0 and 1.0"
    );
    Ok(())
}

/// The kind of a native dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// An informational dialog.
    Info,
    /// A warning dialog.
    Warning,
    /// An error dialog.
    Error,
}

impl DialogKind {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Options for displaying a native dialog.
#[derive(Debug, Clone)]
pub struct DialogOptions {
    /// The kind of dialog to display.
    pub kind: DialogKind,
    /// The title of the dialog.
    pub title: SharedString,
    /// The primary message of the dialog.
    pub message: SharedString,
    /// Optional detail text shown below the message.
    pub detail: Option<SharedString>,
    /// The button labels for the dialog.
    pub buttons: Vec<SharedString>,
    /// Optional default button index for platforms that expose default actions.
    pub default_button: Option<usize>,
    /// Optional cancel button index for escape/close behavior.
    pub cancel_button: Option<usize>,
}

impl DialogOptions {
    /// Number of dialog buttons.
    pub fn button_count(&self) -> usize {
        self.buttons.len()
    }

    /// Whether the dialog has optional detail text.
    pub fn has_detail(&self) -> bool {
        self.detail.is_some()
    }

    /// Whether the dialog has an explicit default action.
    pub fn has_default_button(&self) -> bool {
        self.default_button
            .is_some_and(|index| index < self.button_count())
    }

    /// Whether the dialog has an explicit cancel/escape action.
    pub fn has_cancel_button(&self) -> bool {
        self.cancel_button
            .is_some_and(|index| index < self.button_count())
    }

    /// Returns a content-safe summary for raw dialog options.
    pub fn to_text(&self) -> String {
        format!(
            "dialog options {}: {} buttons, detail {}, default {}, cancel {}",
            self.kind.to_text(),
            self.button_count(),
            self.has_detail(),
            self.has_default_button(),
            self.has_cancel_button()
        )
    }
}

/// Builder for native message/confirmation dialogs.
#[derive(Debug, Clone)]
pub struct MessageDialogBuilder {
    options: DialogOptions,
}

/// Checked, inspectable plan for a native message/confirmation dialog.
#[derive(Debug, Clone)]
pub struct MessageDialogPlan {
    options: DialogOptions,
}

impl MessageDialogPlan {
    /// Dialog kind requested from the platform.
    pub fn kind(&self) -> DialogKind {
        self.options.kind
    }

    /// Dialog title.
    pub fn title(&self) -> &SharedString {
        &self.options.title
    }

    /// Primary dialog message.
    pub fn message(&self) -> &SharedString {
        &self.options.message
    }

    /// Optional detail text.
    pub fn detail(&self) -> Option<&SharedString> {
        self.options.detail.as_ref()
    }

    /// Button labels in returned-index order.
    pub fn buttons(&self) -> &[SharedString] {
        &self.options.buttons
    }

    /// Number of dialog buttons.
    pub fn button_count(&self) -> usize {
        self.options.buttons.len()
    }

    /// Return the index for a button label, if present.
    pub fn button_index(&self, label: &str) -> Option<usize> {
        self.options
            .buttons
            .iter()
            .position(|button| button.as_ref() == label)
    }

    /// Default button index, when configured.
    pub fn default_button_index(&self) -> Option<usize> {
        self.options.default_button
    }

    /// Default button label, when configured.
    pub fn default_button_label(&self) -> Option<&SharedString> {
        self.options
            .default_button
            .and_then(|index| self.options.buttons.get(index))
    }

    /// Cancel button index, when configured.
    pub fn cancel_button_index(&self) -> Option<usize> {
        self.options.cancel_button
    }

    /// Cancel button label, when configured.
    pub fn cancel_button_label(&self) -> Option<&SharedString> {
        self.options
            .cancel_button
            .and_then(|index| self.options.buttons.get(index))
    }

    /// Whether the dialog has an explicit cancel/escape action.
    pub fn has_cancel_button(&self) -> bool {
        self.cancel_button_label().is_some()
    }

    /// Whether the dialog has optional detail text.
    pub fn has_detail(&self) -> bool {
        self.options.detail.is_some()
    }

    /// Returns a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "message dialog {}: {} buttons, detail {}, default {:?}, cancel {:?}",
            self.kind().to_text(),
            self.button_count(),
            self.has_detail(),
            self.default_button_index(),
            self.cancel_button_index()
        )
    }

    /// Return a clone of the raw options for lower-level platform APIs.
    pub fn options(&self) -> DialogOptions {
        self.options.clone()
    }

    /// Consume the plan into raw dialog options.
    pub fn into_options(self) -> DialogOptions {
        self.options
    }
}

impl MessageDialogBuilder {
    /// Create a message dialog with an `OK` button.
    pub fn new(
        kind: DialogKind,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
    ) -> Self {
        Self {
            options: DialogOptions {
                kind,
                title: title.into(),
                message: message.into(),
                detail: None,
                buttons: vec!["OK".into()],
                default_button: Some(0),
                cancel_button: None,
            },
        }
    }

    /// Create an informational message dialog.
    pub fn info(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self::new(DialogKind::Info, title, message)
    }

    /// Create a warning message dialog.
    pub fn warning(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self::new(DialogKind::Warning, title, message)
    }

    /// Create an error message dialog.
    pub fn error(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self::new(DialogKind::Error, title, message)
    }

    /// Create a two-button confirmation dialog.
    ///
    /// The returned button indexes are `0` for Cancel and `1` for OK.
    pub fn confirm(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self::warning(title, message)
            .buttons(["Cancel", "OK"])
            .cancel_button(0)
            .default_button(1)
    }

    /// Create a confirmation dialog for destructive actions.
    ///
    /// The returned button indexes are `0` for Cancel and `1` for the
    /// destructive action.
    pub fn destructive_confirm(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        destructive_label: impl Into<SharedString>,
    ) -> Self {
        Self::warning(title, message)
            .buttons(["Cancel".into(), destructive_label.into()])
            .cancel_button(0)
            .default_button(0)
    }

    /// Create a standard unsaved-changes confirmation dialog.
    ///
    /// The returned button indexes are `0` for Cancel, `1` for Don't Save,
    /// and `2` for Save.
    pub fn save_discard_cancel(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
    ) -> Self {
        Self::warning(title, message)
            .buttons(["Cancel", "Don't Save", "Save"])
            .cancel_button(0)
            .default_button(2)
    }

    /// Set the optional detail text.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.options.detail = Some(detail.into());
        self
    }

    /// Replace the dialog buttons.
    pub fn buttons(mut self, buttons: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.options.buttons = buttons.into_iter().map(Into::into).collect();
        self
    }

    /// Append one button to the dialog.
    pub fn button(mut self, label: impl Into<SharedString>) -> Self {
        self.options.buttons.push(label.into());
        self
    }

    /// Set the default button index.
    pub fn default_button(mut self, index: usize) -> Self {
        self.options.default_button = Some(index);
        self
    }

    /// Set the cancel button index used for escape/close behavior.
    pub fn cancel_button(mut self, index: usize) -> Self {
        self.options.cancel_button = Some(index);
        self
    }

    /// Returns the raw dialog kind.
    pub fn kind(&self) -> DialogKind {
        self.options.kind
    }

    /// Returns the dialog title.
    pub fn title(&self) -> &SharedString {
        &self.options.title
    }

    /// Returns the primary message.
    pub fn message(&self) -> &SharedString {
        &self.options.message
    }

    /// Returns the optional detail text.
    pub fn detail_text(&self) -> Option<&SharedString> {
        self.options.detail.as_ref()
    }

    /// Returns the configured button labels.
    pub fn buttons_list(&self) -> &[SharedString] {
        &self.options.buttons
    }

    /// Returns the configured default button index.
    pub fn default_button_index(&self) -> Option<usize> {
        self.options.default_button
    }

    /// Returns the configured cancel button index.
    pub fn cancel_button_index(&self) -> Option<usize> {
        self.options.cancel_button
    }

    /// Returns a content-safe summary before building or showing the dialog.
    pub fn to_text(&self) -> String {
        self.options.to_text()
    }

    /// Validate required fields before showing the dialog.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_message_dialog_text(&self.options.title, "message dialog title", 256, false)?;
        validate_message_dialog_text(&self.options.message, "message dialog message", 2048, true)?;
        if let Some(detail) = &self.options.detail {
            validate_message_dialog_text(detail, "message dialog detail", 4096, true)?;
        }
        anyhow::ensure!(
            !self.options.buttons.is_empty(),
            "message dialog must contain at least one button"
        );
        anyhow::ensure!(
            self.options.buttons.len() <= 6,
            "message dialog cannot contain more than 6 buttons"
        );
        let mut button_labels = std::collections::HashSet::new();
        for button in &self.options.buttons {
            validate_message_dialog_text(button, "message dialog button label", 128, false)?;
            anyhow::ensure!(
                button_labels.insert(button.as_ref()),
                "message dialog button labels must be unique: {}",
                button
            );
        }
        if let Some(index) = self.options.default_button {
            anyhow::ensure!(
                index < self.options.buttons.len(),
                "message dialog default button index is out of range"
            );
        }
        if let Some(index) = self.options.cancel_button {
            anyhow::ensure!(
                index < self.options.buttons.len(),
                "message dialog cancel button index is out of range"
            );
        }
        Ok(())
    }

    /// Return a clone of the raw options for inspection or lower-level APIs.
    pub fn options(&self) -> DialogOptions {
        self.options.clone()
    }

    /// Build a checked, inspectable dialog plan.
    pub fn build_checked(self) -> Result<MessageDialogPlan> {
        self.validate()?;
        Ok(MessageDialogPlan {
            options: self.options,
        })
    }

    /// Consume the builder into raw dialog options.
    pub fn into_options(self) -> DialogOptions {
        self.options
    }
}

fn validate_message_dialog_text(
    value: &SharedString,
    label: &str,
    max_len: usize,
    allow_multiline: bool,
) -> Result<()> {
    let value = value.as_ref();
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value.trim() == value,
        "{label} cannot have leading or trailing whitespace: {value:?}"
    );
    anyhow::ensure!(
        value.len() <= max_len,
        "{label} cannot be longer than {max_len} bytes"
    );
    anyhow::ensure!(
        !value.chars().any(|character| {
            character.is_control() && !(allow_multiline && matches!(character, '\n' | '\r' | '\t'))
        }),
        "{label} cannot contain control characters"
    );
    Ok(())
}

#[cfg(test)]
mod message_dialog_tests {
    use super::*;

    #[test]
    fn message_dialog_save_discard_cancel_preserves_button_contract() {
        let dialog = MessageDialogBuilder::save_discard_cancel(
            "Save changes?",
            "This document has unsaved changes.",
        );

        assert!(dialog.validate().is_ok());
        assert_eq!(
            dialog.buttons_list(),
            &[
                SharedString::from("Cancel"),
                SharedString::from("Don't Save"),
                SharedString::from("Save")
            ]
        );
        assert_eq!(dialog.cancel_button_index(), Some(0));
        assert_eq!(dialog.default_button_index(), Some(2));

        let plan = dialog.build_checked().unwrap();
        assert_eq!(plan.kind(), DialogKind::Warning);
        assert_eq!(plan.button_count(), 3);
        assert_eq!(plan.button_index("Don't Save"), Some(1));
        assert_eq!(plan.default_button_index(), Some(2));
        assert_eq!(
            plan.default_button_label().map(|label| label.as_ref()),
            Some("Save")
        );
        assert_eq!(plan.cancel_button_index(), Some(0));
        assert_eq!(
            plan.cancel_button_label().map(|label| label.as_ref()),
            Some("Cancel")
        );
        assert!(plan.has_cancel_button());
    }

    #[test]
    fn message_dialog_rejects_ambiguous_generated_copy() {
        assert!(
            MessageDialogBuilder::info(" Title", "Message")
                .validate()
                .is_err()
        );
        assert!(
            MessageDialogBuilder::info("Title", "Message\0")
                .validate()
                .is_err()
        );
        assert!(
            MessageDialogBuilder::info("Title", "Message")
                .detail(" detail")
                .validate()
                .is_err()
        );
        assert!(
            MessageDialogBuilder::info("Title", "Message")
                .buttons(["OK", "OK"])
                .validate()
                .is_err()
        );
        assert!(
            MessageDialogBuilder::info("Title", "Message")
                .buttons(["A", "B", "C", "D", "E", "F", "G"])
                .validate()
                .is_err()
        );
        assert!(
            MessageDialogBuilder::info("Title", "Message")
                .button(" Later")
                .validate()
                .is_err()
        );
    }
}

/// Information about the operating system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    /// The name of the operating system (e.g. "macos", "linux", "windows").
    pub name: SharedString,
    /// The version of the operating system.
    pub version: SharedString,
    /// The CPU architecture (e.g. "x86_64", "aarch64").
    pub arch: SharedString,
    /// The system locale (e.g. "en-US").
    pub locale: SharedString,
    /// The hostname of the system.
    pub hostname: SharedString,
}

/// The kind of biometric authentication available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometricKind {
    /// macOS Touch ID.
    TouchId,
    /// Windows Hello.
    WindowsHello,
    /// Generic fingerprint reader.
    Fingerprint,
}

/// The availability status of biometric authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometricStatus {
    /// Biometric authentication is available with the given kind.
    Available(BiometricKind),
    /// Biometric authentication is not available.
    Unavailable,
}

/// A snapshot of a window's state for save/restore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    /// The window bounds.
    pub bounds: WindowBounds,
    /// The display the window is on.
    pub display_id: Option<DisplayId>,
    /// Whether the window is fullscreen.
    pub fullscreen: bool,
}

/// A semantic window position for positioning windows relative to the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowPosition {
    /// Center the window on the primary display.
    Center,
    /// Center the window on the given display.
    CenterOnDisplay(DisplayId),
    /// Center the window above the tray icon area.
    TrayCenter(Bounds<Pixels>),
    /// Position the window in the top-right corner.
    TopRight {
        /// The margin from the screen edge.
        margin: Pixels,
    },
    /// Position the window in the bottom-right corner.
    BottomRight {
        /// The margin from the screen edge.
        margin: Pixels,
    },
    /// Position the window in the top-left corner.
    TopLeft {
        /// The margin from the screen edge.
        margin: Pixels,
    },
    /// Position the window in the bottom-left corner.
    BottomLeft {
        /// The margin from the screen edge.
        margin: Pixels,
    },
}

/// A notification action button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAction {
    /// A unique identifier for this action, returned in the callback when the user clicks it.
    pub id: String,
    /// The label displayed on the action button.
    pub label: String,
}

/// Delivery urgency for OS-level notifications.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationUrgency {
    /// Low-priority notification that should not interrupt the user.
    Low,
    /// Normal-priority notification.
    #[default]
    Normal,
    /// Critical or time-sensitive notification.
    Critical,
}

impl NotificationUrgency {
    /// Stable lowercase key for logs and platform mappers.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::Critical => "critical",
        }
    }
}

impl NotificationAction {
    /// Create a notification action button.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    /// A conventional action id for opening the related item.
    pub const OPEN_ID: &'static str = "open";

    /// A conventional action id for dismissing or deferring the notification.
    pub const DISMISS_ID: &'static str = "dismiss";

    /// A conventional action id for retrying the related operation.
    pub const RETRY_ID: &'static str = "retry";

    /// A conventional action id for opening related settings or preferences.
    pub const SETTINGS_ID: &'static str = "settings";

    /// Create a conventional "Open" action.
    pub fn open(label: impl Into<String>) -> Self {
        Self::new(Self::OPEN_ID, label)
    }

    /// Create a conventional "Dismiss" action.
    pub fn dismiss(label: impl Into<String>) -> Self {
        Self::new(Self::DISMISS_ID, label)
    }

    /// Create a conventional "Retry" action.
    pub fn retry(label: impl Into<String>) -> Self {
        Self::new(Self::RETRY_ID, label)
    }

    /// Create a conventional "Settings" action.
    pub fn settings(label: impl Into<String>) -> Self {
        Self::new(Self::SETTINGS_ID, label)
    }

    /// Whether this action uses the conventional open id.
    pub fn is_open(&self) -> bool {
        self.id == Self::OPEN_ID
    }

    /// Whether this action uses the conventional dismiss/defer id.
    pub fn is_dismiss(&self) -> bool {
        self.id == Self::DISMISS_ID
    }

    /// Whether this action uses the conventional retry id.
    pub fn is_retry(&self) -> bool {
        self.id == Self::RETRY_ID
    }

    /// Whether this action uses the conventional settings/preferences id.
    pub fn is_settings(&self) -> bool {
        self.id == Self::SETTINGS_ID
    }

    /// Whether this action uses one of Kael's conventional ids.
    pub fn is_conventional(&self) -> bool {
        self.is_open() || self.is_dismiss() || self.is_retry() || self.is_settings()
    }
}

/// Notification feature used when checking how a builder maps to a platform backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationFeature {
    /// Basic notification delivery.
    Basic,
    /// Action buttons and action callbacks.
    Actions,
    /// Delivery urgency / priority.
    Urgency,
    /// Silent delivery.
    Silent,
    /// Replacement tags.
    Tag,
    /// Grouping keys.
    Group,
    /// Timeout / expiration hints.
    Timeout,
}

impl NotificationFeature {
    /// Stable feature key for logs and agent traces.
    pub fn key(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Actions => "actions",
            Self::Urgency => "urgency",
            Self::Silent => "silent",
            Self::Tag => "tag",
            Self::Group => "group",
            Self::Timeout => "timeout",
        }
    }
}

/// Platform/backend notification feature support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NotificationFeatureSupport {
    basic: bool,
    actions: bool,
    urgency: bool,
    silent: bool,
    tag: bool,
    group: bool,
    timeout: bool,
}

impl NotificationFeatureSupport {
    /// No OS-level notification support.
    pub fn unsupported() -> Self {
        Self::default()
    }

    /// Basic title/body delivery only.
    pub fn basic() -> Self {
        Self {
            basic: true,
            ..Self::default()
        }
    }

    /// Basic notifications plus action buttons.
    pub fn actions() -> Self {
        Self::basic().with_actions()
    }

    /// Broad rich-notification support profile.
    pub fn rich() -> Self {
        Self {
            basic: true,
            actions: true,
            urgency: true,
            silent: true,
            tag: true,
            group: true,
            timeout: true,
        }
    }

    /// Mark action buttons as supported.
    pub fn with_actions(mut self) -> Self {
        self.basic = true;
        self.actions = true;
        self
    }

    /// Mark urgency metadata as supported.
    pub fn with_urgency(mut self) -> Self {
        self.basic = true;
        self.urgency = true;
        self
    }

    /// Mark silent-delivery metadata as supported.
    pub fn with_silent(mut self) -> Self {
        self.basic = true;
        self.silent = true;
        self
    }

    /// Mark replacement tags as supported.
    pub fn with_tags(mut self) -> Self {
        self.basic = true;
        self.tag = true;
        self
    }

    /// Mark grouping keys as supported.
    pub fn with_groups(mut self) -> Self {
        self.basic = true;
        self.group = true;
        self
    }

    /// Mark timeout hints as supported.
    pub fn with_timeouts(mut self) -> Self {
        self.basic = true;
        self.timeout = true;
        self
    }

    /// Whether a feature is supported.
    pub fn supports(&self, feature: NotificationFeature) -> bool {
        match feature {
            NotificationFeature::Basic => self.basic,
            NotificationFeature::Actions => self.actions,
            NotificationFeature::Urgency => self.urgency,
            NotificationFeature::Silent => self.silent,
            NotificationFeature::Tag => self.tag,
            NotificationFeature::Group => self.group,
            NotificationFeature::Timeout => self.timeout,
        }
    }

    /// Whether basic OS-level notification delivery is supported.
    pub fn supports_basic(&self) -> bool {
        self.supports(NotificationFeature::Basic)
    }

    /// Whether action buttons are supported.
    pub fn supports_actions(&self) -> bool {
        self.supports(NotificationFeature::Actions)
    }

    /// Whether delivery metadata is fully supported.
    pub fn supports_delivery_metadata(&self) -> bool {
        self.urgency && self.silent && self.tag && self.group && self.timeout
    }

    /// Stable summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "notification support: basic {}, actions {}, urgency {}, silent {}, tag {}, group {}, timeout {}",
            self.basic, self.actions, self.urgency, self.silent, self.tag, self.group, self.timeout
        )
    }
}

/// Checked notification delivery plan against a known backend support profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationDeliveryPlan {
    notification: NotificationBuilder,
    support: NotificationFeatureSupport,
    missing_features: Vec<NotificationFeature>,
}

impl NotificationDeliveryPlan {
    /// Create a checked delivery plan for a notification and support profile.
    pub fn new(
        notification: NotificationBuilder,
        support: NotificationFeatureSupport,
    ) -> Result<Self> {
        notification.validate()?;
        let missing_features = notification.missing_features(support);
        Ok(Self {
            notification,
            support,
            missing_features,
        })
    }

    /// Planned notification.
    pub fn notification(&self) -> &NotificationBuilder {
        &self.notification
    }

    /// Backend support profile used for the plan.
    pub fn support(&self) -> NotificationFeatureSupport {
        self.support
    }

    /// Features requested by the notification but not supported by the profile.
    pub fn missing_features(&self) -> &[NotificationFeature] {
        &self.missing_features
    }

    /// Number of unsupported requested features.
    pub fn missing_feature_count(&self) -> usize {
        self.missing_features.len()
    }

    /// Whether no requested features need fallback handling.
    pub fn is_fully_supported(&self) -> bool {
        self.missing_features.is_empty()
    }

    /// Whether app code should use a fallback route or degrade metadata.
    pub fn requires_fallback(&self) -> bool {
        !self.is_fully_supported()
    }

    /// Content-safe summary for notification delivery planning.
    pub fn to_text(&self) -> String {
        let missing = if self.missing_features.is_empty() {
            "none".to_string()
        } else {
            self.missing_features
                .iter()
                .map(|feature| feature.key())
                .collect::<Vec<_>>()
                .join(", ")
        };

        format!(
            "notification delivery plan: fully supported {}, fallback {}, missing [{}], {}",
            self.is_fully_supported(),
            self.requires_fallback(),
            missing,
            self.notification.to_text()
        )
    }
}

/// Builder for an OS-level notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationBuilder {
    title: String,
    body: String,
    actions: Vec<NotificationAction>,
    #[serde(default)]
    urgency: NotificationUrgency,
    #[serde(default)]
    silent: bool,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

impl NotificationBuilder {
    /// Create a notification with a title and body.
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
            urgency: NotificationUrgency::Normal,
            silent: false,
            tag: None,
            group: None,
            timeout_ms: None,
        }
    }

    /// Add an action button.
    pub fn action(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.actions.push(NotificationAction::new(id, label));
        self
    }

    /// Add a conventional action for opening the related item.
    pub fn open_action(self, label: impl Into<String>) -> Self {
        self.actions([NotificationAction::open(label)])
    }

    /// Add a conventional action for dismissing or deferring the notification.
    pub fn dismiss_action(self, label: impl Into<String>) -> Self {
        self.actions([NotificationAction::dismiss(label)])
    }

    /// Add a conventional action for retrying the related operation.
    pub fn retry_action(self, label: impl Into<String>) -> Self {
        self.actions([NotificationAction::retry(label)])
    }

    /// Add a conventional action for opening related settings or preferences.
    pub fn settings_action(self, label: impl Into<String>) -> Self {
        self.actions([NotificationAction::settings(label)])
    }

    /// Add conventional open and dismiss actions in the platform display order.
    pub fn open_and_dismiss_actions(
        self,
        open_label: impl Into<String>,
        dismiss_label: impl Into<String>,
    ) -> Self {
        self.actions([
            NotificationAction::open(open_label),
            NotificationAction::dismiss(dismiss_label),
        ])
    }

    /// Add several action buttons.
    pub fn actions(mut self, actions: impl IntoIterator<Item = NotificationAction>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// Set the delivery urgency.
    pub fn urgency(mut self, urgency: NotificationUrgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Mark the notification as low-priority.
    pub fn low_priority(self) -> Self {
        self.urgency(NotificationUrgency::Low)
    }

    /// Mark the notification as critical or time-sensitive.
    pub fn critical(self) -> Self {
        self.urgency(NotificationUrgency::Critical)
    }

    /// Request silent delivery when the platform supports it.
    pub fn silent(mut self, silent: bool) -> Self {
        self.silent = silent;
        self
    }

    /// Convenience alias for enabling silent delivery.
    pub fn deliver_silently(self) -> Self {
        self.silent(true)
    }

    /// Set a stable replacement tag for coalescing related notifications.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Set a stable grouping key for related notifications.
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Set a timeout hint in milliseconds.
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Set a timeout hint in whole seconds.
    pub fn timeout_secs(self, timeout_secs: u64) -> Self {
        self.timeout_ms(timeout_secs.saturating_mul(1_000))
    }

    /// The notification title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The notification body.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The action buttons attached to this notification.
    pub fn action_buttons(&self) -> &[NotificationAction] {
        &self.actions
    }

    /// Requested notification urgency.
    pub fn urgency_level(&self) -> NotificationUrgency {
        self.urgency
    }

    /// Whether silent delivery was requested.
    pub fn is_silent(&self) -> bool {
        self.silent
    }

    /// Whether a replacement tag was configured.
    pub fn has_tag(&self) -> bool {
        self.tag.is_some()
    }

    /// Whether a grouping key was configured.
    pub fn has_group(&self) -> bool {
        self.group.is_some()
    }

    /// Whether a timeout hint was configured.
    pub fn has_timeout(&self) -> bool {
        self.timeout_ms.is_some()
    }

    /// Replacement tag, when configured.
    pub fn tag_value(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// Grouping key, when configured.
    pub fn group_value(&self) -> Option<&str> {
        self.group.as_deref()
    }

    /// Timeout hint in milliseconds, when configured.
    pub fn timeout_millis(&self) -> Option<u64> {
        self.timeout_ms
    }

    /// The configured notification action IDs in display order.
    pub fn action_ids(&self) -> impl Iterator<Item = &str> {
        self.actions.iter().map(|action| action.id.as_str())
    }

    /// Number of configured notification action buttons.
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Whether this notification has action buttons.
    pub fn has_actions(&self) -> bool {
        !self.actions.is_empty()
    }

    /// Whether this notification includes a conventional open action.
    pub fn has_open_action(&self) -> bool {
        self.actions.iter().any(NotificationAction::is_open)
    }

    /// Whether this notification includes a conventional dismiss/defer action.
    pub fn has_dismiss_action(&self) -> bool {
        self.actions.iter().any(NotificationAction::is_dismiss)
    }

    /// Whether this notification includes a conventional retry action.
    pub fn has_retry_action(&self) -> bool {
        self.actions.iter().any(NotificationAction::is_retry)
    }

    /// Whether this notification includes a conventional settings/preferences action.
    pub fn has_settings_action(&self) -> bool {
        self.actions.iter().any(NotificationAction::is_settings)
    }

    /// Number of custom action ids.
    pub fn custom_action_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|action| !action.is_conventional())
            .count()
    }

    /// Features this notification requests from a platform backend.
    pub fn requested_features(&self) -> Vec<NotificationFeature> {
        let mut features = vec![NotificationFeature::Basic];
        if self.has_actions() {
            features.push(NotificationFeature::Actions);
        }
        if self.urgency != NotificationUrgency::Normal {
            features.push(NotificationFeature::Urgency);
        }
        if self.is_silent() {
            features.push(NotificationFeature::Silent);
        }
        if self.has_tag() {
            features.push(NotificationFeature::Tag);
        }
        if self.has_group() {
            features.push(NotificationFeature::Group);
        }
        if self.has_timeout() {
            features.push(NotificationFeature::Timeout);
        }
        features
    }

    /// Requested features not supported by the given backend profile.
    pub fn missing_features(
        &self,
        support: NotificationFeatureSupport,
    ) -> Vec<NotificationFeature> {
        self.requested_features()
            .into_iter()
            .filter(|feature| !support.supports(*feature))
            .collect()
    }

    /// Create a checked delivery plan against a known backend support profile.
    pub fn delivery_plan(
        self,
        support: NotificationFeatureSupport,
    ) -> Result<NotificationDeliveryPlan> {
        NotificationDeliveryPlan::new(self, support)
    }

    /// Returns a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "notification: urgency {}, silent {}, tag {}, group {}, timeout {}, actions {}, has actions {}, open {}, dismiss {}, retry {}, settings {}, custom {}",
            self.urgency.key(),
            self.silent,
            self.has_tag(),
            self.has_group(),
            self.has_timeout(),
            self.action_count(),
            self.has_actions(),
            self.has_open_action(),
            self.has_dismiss_action(),
            self.has_retry_action(),
            self.has_settings_action(),
            self.custom_action_count()
        )
    }

    /// Validate the notification before dispatching it to the platform backend.
    pub fn validate(&self) -> Result<()> {
        validate_notification_title(&self.title)?;
        validate_notification_body(&self.body)?;
        if let Some(tag) = &self.tag {
            validate_notification_metadata(tag, "notification tag", 128)?;
        }
        if let Some(group) = &self.group {
            validate_notification_metadata(group, "notification group", 128)?;
        }
        if let Some(timeout_ms) = self.timeout_ms {
            anyhow::ensure!(
                timeout_ms > 0,
                "notification timeout must be greater than zero"
            );
            anyhow::ensure!(
                timeout_ms <= 604_800_000,
                "notification timeout cannot be longer than 7 days"
            );
        }

        anyhow::ensure!(
            self.actions.len() <= 4,
            "notification cannot have more than 4 action buttons"
        );

        for action in &self.actions {
            validate_notification_action_id(&action.id)?;
            validate_notification_action_label(&action.label)?;

            if self
                .actions
                .iter()
                .filter(|candidate| candidate.id == action.id)
                .count()
                > 1
            {
                anyhow::bail!("notification action id must be unique: {}", action.id);
            }
        }

        Ok(())
    }

    /// Convert this builder into its raw platform parts.
    pub fn into_parts(self) -> (String, String, Vec<NotificationAction>) {
        (self.title, self.body, self.actions)
    }
}

fn validate_notification_title(title: &str) -> Result<()> {
    validate_notification_text(title, "notification title", 256, false)
}

fn validate_notification_body(body: &str) -> Result<()> {
    validate_notification_text(body, "notification body", 2048, true)
}

fn validate_notification_action_label(label: &str) -> Result<()> {
    validate_notification_text(label, "notification action label", 128, false)
}

fn validate_notification_metadata(value: &str, label: &str, max_len: usize) -> Result<()> {
    validate_notification_text(value, label, max_len, false)
}

fn validate_notification_text(
    value: &str,
    label: &str,
    max_len: usize,
    allow_multiline: bool,
) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value.trim() == value,
        "{label} cannot have leading or trailing whitespace: {value:?}"
    );
    anyhow::ensure!(
        value.len() <= max_len,
        "{label} cannot be longer than {max_len} bytes"
    );
    anyhow::ensure!(
        !value.chars().any(|character| {
            character.is_control() && !(allow_multiline && matches!(character, '\n' | '\r' | '\t'))
        }),
        "{label} cannot contain control characters"
    );
    Ok(())
}

fn validate_notification_action_id(id: &str) -> Result<()> {
    anyhow::ensure!(
        !id.trim().is_empty(),
        "notification action id cannot be empty"
    );
    anyhow::ensure!(
        id.trim() == id,
        "notification action id cannot have leading or trailing whitespace: {id:?}"
    );
    anyhow::ensure!(
        id.len() <= 64,
        "notification action id cannot be longer than 64 bytes: {id:?}"
    );
    anyhow::ensure!(
        id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        }),
        "notification action id may only contain ASCII letters, digits, '-', '_', '.', or ':': {id:?}"
    );
    Ok(())
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn notification_builder_common_actions_validate() {
        let notification = NotificationBuilder::new("Sync failed", "Could not reach the server")
            .retry_action("Retry")
            .settings_action("Settings")
            .dismiss_action("Later");

        assert!(notification.validate().is_ok());
        assert_eq!(
            notification.action_ids().collect::<Vec<_>>(),
            vec!["retry", "settings", "dismiss"]
        );

        let notification = NotificationBuilder::new("Update available", "Version 2.0 is ready")
            .open_and_dismiss_actions("Install", "Later");
        assert_eq!(
            notification.action_buttons(),
            &[
                NotificationAction::open("Install"),
                NotificationAction::dismiss("Later")
            ]
        );
    }

    #[test]
    fn notification_builder_delivery_metadata_validates() {
        let notification = NotificationBuilder::new("Sync complete", "3 files uploaded")
            .low_priority()
            .deliver_silently()
            .tag("workspace-a")
            .group("sync")
            .timeout_ms(5_000);

        assert!(notification.validate().is_ok());
        assert_eq!(notification.urgency_level(), NotificationUrgency::Low);
        assert!(notification.is_silent());
        assert!(notification.has_tag());
        assert!(notification.has_group());
        assert!(notification.has_timeout());
        assert_eq!(notification.tag_value(), Some("workspace-a"));
        assert_eq!(notification.group_value(), Some("sync"));
        assert_eq!(notification.timeout_millis(), Some(5_000));
        assert_eq!(
            notification.to_text(),
            "notification: urgency low, silent true, tag true, group true, timeout true, actions 0, has actions false, open false, dismiss false, retry false, settings false, custom 0"
        );
        assert!(!notification.to_text().contains("workspace-a"));
        assert!(!notification.to_text().contains("5000"));
    }

    #[test]
    fn notification_delivery_plan_reports_platform_variance() {
        let notification = NotificationBuilder::new("Update available", "Version 2.0")
            .critical()
            .deliver_silently()
            .tag("update")
            .group("updates")
            .timeout_secs(30)
            .open_and_dismiss_actions("Install", "Later");

        assert_eq!(
            notification.requested_features(),
            vec![
                NotificationFeature::Basic,
                NotificationFeature::Actions,
                NotificationFeature::Urgency,
                NotificationFeature::Silent,
                NotificationFeature::Tag,
                NotificationFeature::Group,
                NotificationFeature::Timeout,
            ]
        );
        assert_eq!(NotificationFeature::Actions.key(), "actions");

        let rich = NotificationFeatureSupport::rich();
        assert!(rich.supports_basic());
        assert!(rich.supports_actions());
        assert!(rich.supports_delivery_metadata());
        assert_eq!(
            rich.to_text(),
            "notification support: basic true, actions true, urgency true, silent true, tag true, group true, timeout true"
        );

        let plan = notification.clone().delivery_plan(rich).unwrap();
        assert!(plan.is_fully_supported());
        assert!(!plan.requires_fallback());
        assert_eq!(plan.missing_feature_count(), 0);
        assert_eq!(plan.support(), rich);
        assert_eq!(plan.notification().action_count(), 2);

        let basic = NotificationFeatureSupport::basic();
        assert!(basic.supports_basic());
        assert!(!basic.supports_actions());
        assert!(!basic.supports_delivery_metadata());
        let plan = notification.delivery_plan(basic).unwrap();
        assert!(!plan.is_fully_supported());
        assert!(plan.requires_fallback());
        assert_eq!(
            plan.missing_features(),
            &[
                NotificationFeature::Actions,
                NotificationFeature::Urgency,
                NotificationFeature::Silent,
                NotificationFeature::Tag,
                NotificationFeature::Group,
                NotificationFeature::Timeout,
            ]
        );
        assert!(plan.to_text().contains("fallback true"));
        assert!(
            plan.to_text()
                .contains("missing [actions, urgency, silent, tag, group, timeout]")
        );
    }

    #[test]
    fn notification_delivery_plan_summary_is_content_safe() {
        let notification = NotificationBuilder::new("Secret project", "Token expired")
            .retry_action("Retry secret sync")
            .tag("project-alpha")
            .group("private-sync")
            .timeout_ms(42_000);
        let plan = notification
            .delivery_plan(NotificationFeatureSupport::unsupported())
            .unwrap();

        assert_eq!(plan.missing_feature_count(), 5);
        assert_eq!(
            plan.missing_features(),
            &[
                NotificationFeature::Basic,
                NotificationFeature::Actions,
                NotificationFeature::Tag,
                NotificationFeature::Group,
                NotificationFeature::Timeout,
            ]
        );
        let summary = plan.to_text();
        assert!(summary.contains("notification delivery plan"));
        assert!(summary.contains("fallback true"));
        assert!(summary.contains("basic"));
        assert!(summary.contains("actions"));
        assert!(summary.contains("timeout"));
        assert!(!summary.contains("Secret project"));
        assert!(!summary.contains("Token expired"));
        assert!(!summary.contains("Retry secret sync"));
        assert!(!summary.contains("project-alpha"));
        assert!(!summary.contains("private-sync"));
        assert!(!summary.contains("42000"));
    }

    #[test]
    fn notification_builder_rejects_ambiguous_generated_copy() {
        assert!(
            NotificationBuilder::new(" Build", "Done")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done\0")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .action("bad id", "Open")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .action("open", " Open")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .action("open", "Open")
                .action("open", "Open again")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .actions([
                    NotificationAction::new("a", "A"),
                    NotificationAction::new("b", "B"),
                    NotificationAction::new("c", "C"),
                    NotificationAction::new("d", "D"),
                    NotificationAction::new("e", "E"),
                ])
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .tag("")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .group(" group")
                .validate()
                .is_err()
        );
        assert!(
            NotificationBuilder::new("Build", "Done")
                .timeout_ms(604_800_001)
                .validate()
                .is_err()
        );
    }
}

/// Information collected for a crash report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    /// The error message.
    pub message: String,
    /// The backtrace at the time of the crash.
    pub backtrace: String,
    /// Information about the operating system.
    pub os_info: OsInfo,
    /// The application version, if available.
    pub app_version: Option<String>,
}

/// A named file-extension filter for native file dialogs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDialogFilter {
    /// User-facing filter name.
    pub name: SharedString,
    /// File extensions without leading dots, such as `png` or `pdf`.
    pub extensions: Vec<SharedString>,
}

impl FileDialogFilter {
    /// Create a named extension filter.
    pub fn new(
        name: impl Into<SharedString>,
        extensions: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self {
            name: name.into(),
            extensions: extensions
                .into_iter()
                .map(|extension| normalize_file_dialog_extension(extension.into()))
                .collect(),
        }
    }

    /// Match common image files.
    pub fn images() -> Self {
        Self::new(
            "Images",
            ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff"],
        )
    }

    /// Match common audio files.
    pub fn audio() -> Self {
        Self::new("Audio", ["mp3", "wav", "flac", "aac", "ogg", "m4a"])
    }

    /// Match common video files.
    pub fn video() -> Self {
        Self::new("Video", ["mp4", "mov", "mkv", "webm", "avi", "m4v"])
    }

    /// Match PDF documents.
    pub fn pdf() -> Self {
        Self::new("PDF", ["pdf"])
    }

    /// Match common text documents.
    pub fn text() -> Self {
        Self::new(
            "Text",
            ["txt", "md", "markdown", "log", "csv", "json", "toml"],
        )
    }

    /// Return the filter name.
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    /// Return normalized extensions without leading dots.
    pub fn extensions(&self) -> &[SharedString] {
        &self.extensions
    }

    /// Number of normalized extensions accepted by this filter.
    pub fn extension_count(&self) -> usize {
        self.extensions.len()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "file dialog filter: name {}, extensions {}",
            !self.name.as_ref().is_empty(),
            self.extension_count()
        )
    }

    /// Validate the filter name and extensions.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.name.as_ref().trim().is_empty(),
            "file dialog filter name cannot be empty"
        );
        anyhow::ensure!(
            !self.extensions.is_empty(),
            "file dialog filter must include at least one extension"
        );
        anyhow::ensure!(
            self.extensions
                .iter()
                .all(|extension| !extension.as_ref().trim().is_empty()),
            "file dialog filter extensions cannot be empty"
        );
        Ok(())
    }
}

fn normalize_file_dialog_extension(extension: SharedString) -> SharedString {
    extension
        .as_ref()
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .into()
}

/// The options that can be configured for a file dialog prompt
#[derive(Clone, Debug)]
pub struct PathPromptOptions {
    /// Should the prompt allow files to be selected?
    pub files: bool,
    /// Should the prompt allow directories to be selected?
    pub directories: bool,
    /// Should the prompt allow multiple files to be selected?
    pub multiple: bool,
    /// The prompt to show to a user when selecting a path
    pub prompt: Option<SharedString>,
    /// Named extension filters shown by file-capable dialogs.
    pub filters: Vec<FileDialogFilter>,
}

impl PathPromptOptions {
    /// Number of configured extension filters.
    pub fn filter_count(&self) -> usize {
        self.filters.len()
    }

    /// Total number of extensions across all configured filters.
    pub fn filter_extension_count(&self) -> usize {
        self.filters
            .iter()
            .map(FileDialogFilter::extension_count)
            .sum()
    }

    /// Whether files can be selected.
    pub fn allows_files(&self) -> bool {
        self.files
    }

    /// Whether directories can be selected.
    pub fn allows_directories(&self) -> bool {
        self.directories
    }

    /// Whether multiple paths can be selected.
    pub fn allows_multiple(&self) -> bool {
        self.multiple
    }

    /// Whether a prompt label is configured.
    pub fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// Returns a path-safe summary for raw path prompt options.
    pub fn to_text(&self) -> String {
        let mode = match (self.allows_files(), self.allows_directories()) {
            (true, true) => "files-and-directories",
            (true, false) => "files",
            (false, true) => "directories",
            (false, false) => "none",
        };

        format!(
            "path prompt options: mode {mode}, multiple {}, prompt {}, {} filters, {} extensions",
            self.allows_multiple(),
            self.has_prompt(),
            self.filter_count(),
            self.filter_extension_count()
        )
    }
}

/// Builder for native open-file/open-directory dialogs.
#[derive(Clone, Debug)]
pub struct OpenDialogBuilder {
    options: PathPromptOptions,
}

/// Checked, inspectable plan for a native open-file/open-directory dialog.
#[derive(Clone, Debug)]
pub struct OpenDialogPlan {
    options: PathPromptOptions,
}

impl OpenDialogPlan {
    /// Return the path prompt options that will be passed to the platform.
    pub fn options(&self) -> &PathPromptOptions {
        &self.options
    }

    /// Whether files can be selected.
    pub fn allows_files(&self) -> bool {
        self.options.files
    }

    /// Whether directories can be selected.
    pub fn allows_directories(&self) -> bool {
        self.options.directories
    }

    /// Whether multiple paths can be selected.
    pub fn allows_multiple(&self) -> bool {
        self.options.multiple
    }

    /// Prompt label shown by the native picker, if configured.
    pub fn prompt(&self) -> Option<&SharedString> {
        self.options.prompt.as_ref()
    }

    /// Named extension filters shown by file-capable dialogs.
    pub fn filters(&self) -> &[FileDialogFilter] {
        &self.options.filters
    }

    /// Number of configured extension filters.
    pub fn filter_count(&self) -> usize {
        self.options.filters.len()
    }

    /// Total number of extensions across all configured filters.
    pub fn filter_extension_count(&self) -> usize {
        self.options
            .filters
            .iter()
            .map(FileDialogFilter::extension_count)
            .sum()
    }

    /// Filter names in configured order.
    pub fn filter_names(&self) -> Vec<&str> {
        self.options
            .filters
            .iter()
            .map(|filter| filter.name.as_ref())
            .collect()
    }

    /// Returns a path-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        let mode = match (self.allows_files(), self.allows_directories()) {
            (true, true) => "files-and-directories",
            (true, false) => "files",
            (false, true) => "directories",
            (false, false) => "none",
        };

        format!(
            "open dialog: mode {mode}, multiple {}, prompt {}, {} filters, {} extensions",
            self.allows_multiple(),
            self.prompt().is_some(),
            self.filter_count(),
            self.filter_extension_count()
        )
    }

    /// Consume the plan into platform path prompt options.
    pub fn into_options(self) -> PathPromptOptions {
        self.options
    }
}

impl OpenDialogBuilder {
    /// Create a dialog that selects a single file.
    pub fn new() -> Self {
        Self {
            options: PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
                filters: Vec::new(),
            },
        }
    }

    /// Create a dialog that selects a single file.
    pub fn file() -> Self {
        Self::new()
    }

    /// Create a dialog that selects multiple files.
    pub fn files() -> Self {
        Self::new().multiple(true)
    }

    /// Create a dialog that selects a single directory.
    pub fn directory() -> Self {
        Self::new().files_allowed(false).directories_allowed(true)
    }

    /// Create a dialog that selects multiple directories.
    pub fn directories() -> Self {
        Self::directory().multiple(true)
    }

    /// Set whether files can be selected.
    pub fn files_allowed(mut self, files: bool) -> Self {
        self.options.files = files;
        self
    }

    /// Set whether directories can be selected.
    pub fn directories_allowed(mut self, directories: bool) -> Self {
        self.options.directories = directories;
        self
    }

    /// Alias for [`Self::directories_allowed`].
    pub fn allow_directories(self, directories: bool) -> Self {
        self.directories_allowed(directories)
    }

    /// Set whether multiple paths can be selected.
    pub fn multiple(mut self, multiple: bool) -> Self {
        self.options.multiple = multiple;
        self
    }

    /// Set the native dialog prompt label.
    pub fn prompt(mut self, prompt: impl Into<SharedString>) -> Self {
        self.options.prompt = Some(prompt.into());
        self
    }

    /// Add a named extension filter.
    pub fn filter(
        mut self,
        name: impl Into<SharedString>,
        extensions: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.options
            .filters
            .push(FileDialogFilter::new(name, extensions));
        self
    }

    /// Add an already-built extension filter.
    pub fn file_filter(mut self, filter: FileDialogFilter) -> Self {
        self.options.filters.push(filter);
        self
    }

    /// Add a common image-file filter.
    pub fn image_files(self) -> Self {
        self.file_filter(FileDialogFilter::images())
    }

    /// Add a common audio-file filter.
    pub fn audio_files(self) -> Self {
        self.file_filter(FileDialogFilter::audio())
    }

    /// Add a common video-file filter.
    pub fn video_files(self) -> Self {
        self.file_filter(FileDialogFilter::video())
    }

    /// Add a PDF filter.
    pub fn pdf_files(self) -> Self {
        self.file_filter(FileDialogFilter::pdf())
    }

    /// Add a common text-file filter.
    pub fn text_files(self) -> Self {
        self.file_filter(FileDialogFilter::text())
    }

    /// Validate required dialog options before showing the dialog.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.options.files || self.options.directories,
            "open dialog must allow files, directories, or both"
        );
        if let Some(prompt) = &self.options.prompt {
            validate_file_dialog_text(prompt, "open dialog prompt", 256)?;
        }
        for filter in &self.options.filters {
            filter.validate()?;
        }
        Ok(())
    }

    /// Return the underlying path prompt options.
    pub fn options(&self) -> &PathPromptOptions {
        &self.options
    }

    /// Returns a path-safe summary before building or showing the dialog.
    pub fn to_text(&self) -> String {
        self.options.to_text()
    }

    /// Build a checked, inspectable open-dialog plan.
    pub fn build_checked(self) -> Result<OpenDialogPlan> {
        self.validate()?;
        Ok(OpenDialogPlan {
            options: self.options,
        })
    }

    /// Consume the builder into path prompt options.
    pub fn into_options(self) -> PathPromptOptions {
        self.options
    }
}

impl Default for OpenDialogBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<OpenDialogBuilder> for PathPromptOptions {
    fn from(value: OpenDialogBuilder) -> Self {
        value.into_options()
    }
}

/// Builder for native save dialogs.
#[derive(Clone, Debug)]
pub struct SaveDialogBuilder {
    directory: PathBuf,
    suggested_name: Option<String>,
    default_extension: Option<SharedString>,
}

/// Checked, inspectable plan for a native save dialog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveDialogPlan {
    directory: PathBuf,
    suggested_name: Option<String>,
    default_extension: Option<SharedString>,
    appended_default_extension: bool,
}

impl SaveDialogPlan {
    /// Initial directory for the save dialog.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Suggested filename after applying the default extension, if configured.
    pub fn suggested_name(&self) -> Option<&str> {
        self.suggested_name.as_deref()
    }

    /// Default extension used to complete extension-less suggested names.
    pub fn default_extension(&self) -> Option<&str> {
        self.default_extension
            .as_ref()
            .map(|extension| extension.as_ref())
    }

    /// Whether the suggested filename was completed by the default extension.
    pub fn appended_default_extension(&self) -> bool {
        self.appended_default_extension
    }

    /// Whether a suggested filename is configured.
    pub fn has_suggested_name(&self) -> bool {
        self.suggested_name.is_some()
    }

    /// Whether a default extension is configured.
    pub fn has_default_extension(&self) -> bool {
        self.default_extension.is_some()
    }

    /// Returns a path-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "save dialog: directory set {}, suggested name {}, default extension {}, appended default extension {}",
            !self.directory.as_os_str().is_empty(),
            self.has_suggested_name(),
            self.has_default_extension(),
            self.appended_default_extension()
        )
    }

    /// Consume the plan into raw save-dialog parts.
    pub fn into_parts(self) -> (PathBuf, Option<String>) {
        (self.directory, self.suggested_name)
    }
}

impl SaveDialogBuilder {
    /// Create a save dialog rooted at a directory.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            suggested_name: None,
            default_extension: None,
        }
    }

    /// Set the suggested filename.
    pub fn suggested_name(mut self, suggested_name: impl Into<String>) -> Self {
        self.suggested_name = Some(suggested_name.into());
        self
    }

    /// Set the extension appended when the suggested name has none.
    pub fn default_extension(mut self, extension: impl Into<SharedString>) -> Self {
        self.default_extension = Some(normalize_file_dialog_extension(extension.into()));
        self
    }

    /// Use `pdf` as the default extension.
    pub fn pdf(self) -> Self {
        self.default_extension("pdf")
    }

    /// Use `txt` as the default extension.
    pub fn text(self) -> Self {
        self.default_extension("txt")
    }

    /// Use `json` as the default extension.
    pub fn json(self) -> Self {
        self.default_extension("json")
    }

    /// The initial directory used by the dialog.
    pub fn directory_path(&self) -> &Path {
        &self.directory
    }

    /// The suggested filename, if configured.
    pub fn suggested_name_value(&self) -> Option<&str> {
        self.suggested_name.as_deref()
    }

    /// The default extension, if configured.
    pub fn default_extension_value(&self) -> Option<&str> {
        self.default_extension
            .as_ref()
            .map(|extension| extension.as_ref())
    }

    /// Validate the save dialog options.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.directory.as_os_str().is_empty(),
            "save dialog directory cannot be empty"
        );
        if let Some(name) = &self.suggested_name {
            validate_save_dialog_suggested_name(name)?;
        }
        if let Some(extension) = &self.default_extension {
            anyhow::ensure!(
                !extension.as_ref().trim().is_empty(),
                "save dialog default extension cannot be empty"
            );
            validate_file_dialog_extension(extension)?;
        }
        Ok(())
    }

    /// Build a checked, inspectable save-dialog plan.
    pub fn build_checked(self) -> Result<SaveDialogPlan> {
        self.validate()?;
        let extension = self
            .default_extension
            .as_ref()
            .map(|extension| extension.as_ref().to_string());
        let appended_default_extension = self
            .suggested_name
            .as_deref()
            .is_some_and(|name| extension.is_some() && Path::new(name).extension().is_none());
        let suggested_name = self
            .suggested_name
            .map(|name| append_default_extension(name, extension.as_deref()));
        Ok(SaveDialogPlan {
            directory: self.directory,
            suggested_name,
            default_extension: self.default_extension,
            appended_default_extension,
        })
    }

    /// Consume the builder into the raw save dialog parts.
    pub fn into_parts(self) -> (PathBuf, Option<String>) {
        let extension = self
            .default_extension
            .as_ref()
            .map(|extension| extension.as_ref().to_string());
        let suggested_name = self
            .suggested_name
            .map(|name| append_default_extension(name, extension.as_deref()));
        (self.directory, suggested_name)
    }
}

fn append_default_extension(name: String, extension: Option<&str>) -> String {
    let Some(extension) = extension else {
        return name;
    };
    if extension.is_empty() || Path::new(&name).extension().is_some() {
        name
    } else {
        format!("{name}.{extension}")
    }
}

fn validate_file_dialog_text(value: &str, label: &str, max_len: usize) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.chars().count() <= max_len,
        "{label} cannot be longer than {max_len} characters"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

fn validate_save_dialog_suggested_name(name: &str) -> Result<()> {
    validate_file_dialog_text(name, "save dialog suggested name", 255)?;
    anyhow::ensure!(
        !name.contains(['/', '\\']),
        "save dialog suggested name cannot contain path separators"
    );
    anyhow::ensure!(
        name != "." && name != "..",
        "save dialog suggested name cannot be a relative path segment"
    );
    Ok(())
}

fn validate_file_dialog_extension(extension: &str) -> Result<()> {
    anyhow::ensure!(
        extension
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+')),
        "save dialog default extension must contain only ASCII letters, numbers, '-', '_' or '+'"
    );
    Ok(())
}

/// What kind of prompt styling to show
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PromptLevel {
    /// A prompt that is shown when the user should be notified of something
    Info,

    /// A prompt that is shown when the user needs to be warned of a potential problem
    Warning,

    /// A prompt that is shown when a critical problem has occurred
    Critical,
}

/// Prompt Button
#[derive(Clone, Debug, PartialEq)]
pub enum PromptButton {
    /// Ok button
    Ok(SharedString),
    /// Cancel button
    Cancel(SharedString),
    /// Other button
    Other(SharedString),
}

impl PromptButton {
    /// Create a button with label
    pub fn new(label: impl Into<SharedString>) -> Self {
        PromptButton::Other(label.into())
    }

    /// Create an Ok button
    pub fn ok(label: impl Into<SharedString>) -> Self {
        PromptButton::Ok(label.into())
    }

    /// Create a Cancel button
    pub fn cancel(label: impl Into<SharedString>) -> Self {
        PromptButton::Cancel(label.into())
    }

    #[allow(dead_code)]
    pub(crate) fn is_cancel(&self) -> bool {
        matches!(self, PromptButton::Cancel(_))
    }

    /// Returns the label of the button
    pub fn label(&self) -> &SharedString {
        match self {
            PromptButton::Ok(label) => label,
            PromptButton::Cancel(label) => label,
            PromptButton::Other(label) => label,
        }
    }
}

impl From<&str> for PromptButton {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "ok" => PromptButton::Ok("Ok".into()),
            "cancel" => PromptButton::Cancel("Cancel".into()),
            _ => PromptButton::Other(SharedString::from(value.to_owned())),
        }
    }
}

/// The style of the cursor (pointer)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum CursorStyle {
    /// The default cursor
    #[default]
    Arrow,

    /// A text input cursor
    /// corresponds to the CSS cursor value `text`
    IBeam,

    /// A crosshair cursor
    /// corresponds to the CSS cursor value `crosshair`
    Crosshair,

    /// A closed hand cursor
    /// corresponds to the CSS cursor value `grabbing`
    ClosedHand,

    /// An open hand cursor
    /// corresponds to the CSS cursor value `grab`
    OpenHand,

    /// A pointing hand cursor
    /// corresponds to the CSS cursor value `pointer`
    PointingHand,

    /// A resize left cursor
    /// corresponds to the CSS cursor value `w-resize`
    ResizeLeft,

    /// A resize right cursor
    /// corresponds to the CSS cursor value `e-resize`
    ResizeRight,

    /// A resize cursor to the left and right
    /// corresponds to the CSS cursor value `ew-resize`
    ResizeLeftRight,

    /// A resize up cursor
    /// corresponds to the CSS cursor value `n-resize`
    ResizeUp,

    /// A resize down cursor
    /// corresponds to the CSS cursor value `s-resize`
    ResizeDown,

    /// A resize cursor directing up and down
    /// corresponds to the CSS cursor value `ns-resize`
    ResizeUpDown,

    /// A resize cursor directing up-left and down-right
    /// corresponds to the CSS cursor value `nesw-resize`
    ResizeUpLeftDownRight,

    /// A resize cursor directing up-right and down-left
    /// corresponds to the CSS cursor value `nwse-resize`
    ResizeUpRightDownLeft,

    /// A cursor indicating that the item/column can be resized horizontally.
    /// corresponds to the CSS cursor value `col-resize`
    ResizeColumn,

    /// A cursor indicating that the item/row can be resized vertically.
    /// corresponds to the CSS cursor value `row-resize`
    ResizeRow,

    /// A text input cursor for vertical layout
    /// corresponds to the CSS cursor value `vertical-text`
    IBeamCursorForVerticalLayout,

    /// A cursor indicating that the operation is not allowed
    /// corresponds to the CSS cursor value `not-allowed`
    OperationNotAllowed,

    /// A cursor indicating that the operation will result in a link
    /// corresponds to the CSS cursor value `alias`
    DragLink,

    /// A cursor indicating that the operation will result in a copy
    /// corresponds to the CSS cursor value `copy`
    DragCopy,

    /// A cursor indicating that the operation will result in a context menu
    /// corresponds to the CSS cursor value `context-menu`
    ContextualMenu,

    /// Hide the cursor
    None,
}

/// A clipboard item that should be copied to the clipboard
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardItem {
    entries: Vec<ClipboardEntry>,
}

/// Metadata attached to an HTML clipboard string.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClipboardHtmlMetadata {
    /// Metadata discriminator for rich HTML clipboard entries.
    pub kind: String,
    /// The HTML fragment associated with the plain-text fallback.
    pub html: String,
}

impl ClipboardHtmlMetadata {
    /// Create metadata for an HTML clipboard entry.
    pub fn new(html: impl Into<String>) -> Result<Self> {
        let html = html.into();
        validate_clipboard_html(&html)?;
        Ok(Self {
            kind: "html".to_string(),
            html,
        })
    }
}

/// Either a ClipboardString or a ClipboardImage
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardEntry {
    /// A string entry
    String(ClipboardString),
    /// An image entry
    Image(Image),
}

impl ClipboardItem {
    /// Create a builder for multi-entry clipboard payloads.
    pub fn builder() -> ClipboardItemBuilder {
        ClipboardItemBuilder::new()
    }

    /// Create a new ClipboardItem::String with no associated metadata
    pub fn new_string(text: String) -> Self {
        Self {
            entries: vec![ClipboardEntry::String(ClipboardString::new(text))],
        }
    }

    /// Create a new ClipboardItem::String with the given text and associated metadata
    pub fn new_string_with_metadata(text: String, metadata: String) -> Self {
        Self {
            entries: vec![ClipboardEntry::String(ClipboardString {
                text,
                metadata: Some(metadata),
            })],
        }
    }

    /// Create a new ClipboardItem::String with the given text and associated metadata
    pub fn new_string_with_json_metadata<T: Serialize>(text: String, metadata: T) -> Self {
        Self {
            entries: vec![ClipboardEntry::String(
                ClipboardString::new(text).with_json_metadata(metadata),
            )],
        }
    }

    /// Create a new ClipboardItem::Image with the given image with no associated metadata
    pub fn new_image(image: &Image) -> Self {
        Self {
            entries: vec![ClipboardEntry::Image(image.clone())],
        }
    }

    /// Create a clipboard item from one or more entries.
    pub fn from_entries(entries: impl IntoIterator<Item = ClipboardEntry>) -> Result<Self> {
        let entries = entries.into_iter().collect::<Vec<_>>();
        anyhow::ensure!(
            !entries.is_empty(),
            "clipboard item must contain at least one entry"
        );
        Ok(Self { entries })
    }

    /// Concatenates together all the ClipboardString entries in the item.
    /// Returns None if there were no ClipboardString entries.
    pub fn text(&self) -> Option<String> {
        let mut answer = String::new();
        let mut any_entries = false;

        for entry in self.entries.iter() {
            if let ClipboardEntry::String(ClipboardString { text, metadata: _ }) = entry {
                answer.push_str(text);
                any_entries = true;
            }
        }

        if any_entries { Some(answer) } else { None }
    }

    /// If this item is one ClipboardEntry::String, returns its metadata.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn metadata(&self) -> Option<&String> {
        match self.entries().first() {
            Some(ClipboardEntry::String(clipboard_string)) if self.entries.len() == 1 => {
                clipboard_string.metadata.as_ref()
            }
            _ => None,
        }
    }

    /// Get the item's entries
    pub fn entries(&self) -> &[ClipboardEntry] {
        &self.entries
    }

    /// Number of entries in this clipboard item.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over string entries.
    pub fn strings(&self) -> impl Iterator<Item = &ClipboardString> {
        self.entries.iter().filter_map(|entry| match entry {
            ClipboardEntry::String(string) => Some(string),
            ClipboardEntry::Image(_) => None,
        })
    }

    /// Iterate over image entries.
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        self.entries.iter().filter_map(|entry| match entry {
            ClipboardEntry::String(_) => None,
            ClipboardEntry::Image(image) => Some(image),
        })
    }

    /// Number of text entries in this clipboard item.
    pub fn text_count(&self) -> usize {
        self.strings().count()
    }

    /// Number of image entries in this clipboard item.
    pub fn image_count(&self) -> usize {
        self.images().count()
    }

    /// Number of string entries that carry metadata.
    pub fn metadata_count(&self) -> usize {
        self.strings()
            .filter(|string| string.metadata().is_some())
            .count()
    }

    /// Total UTF-8 byte length of all text entries.
    pub fn text_len_bytes(&self) -> usize {
        self.strings().map(|string| string.text().len()).sum()
    }

    /// Return the first image entry, if any.
    pub fn first_image(&self) -> Option<&Image> {
        self.images().next()
    }

    /// Return true when any text entry exists.
    pub fn has_text(&self) -> bool {
        self.strings().next().is_some()
    }

    /// Return true when any image entry exists.
    pub fn has_image(&self) -> bool {
        self.first_image().is_some()
    }

    /// Return the first HTML string entry, if any.
    pub fn html(&self) -> Option<String> {
        self.strings().find_map(ClipboardString::html)
    }

    /// Return true when any string entry carries HTML metadata.
    pub fn has_html(&self) -> bool {
        self.html().is_some()
    }

    /// Returns a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "clipboard item: {} entries, {} text, {} images, html {}, metadata {}, text {} bytes",
            self.entry_count(),
            self.text_count(),
            self.image_count(),
            self.has_html(),
            self.metadata_count(),
            self.text_len_bytes()
        )
    }

    /// Decode the metadata of a single string entry as JSON.
    pub fn metadata_json<T>(&self) -> Option<T>
    where
        T: for<'a> Deserialize<'a>,
    {
        match self.entries().first() {
            Some(ClipboardEntry::String(clipboard_string)) if self.entries.len() == 1 => {
                clipboard_string.metadata_json()
            }
            _ => None,
        }
    }

    /// Get owned versions of the item's entries
    pub fn into_entries(self) -> impl Iterator<Item = ClipboardEntry> {
        self.entries.into_iter()
    }
}

/// Builder for rich clipboard payloads containing text, metadata, and images.
#[derive(Clone, Debug, Default)]
pub struct ClipboardItemBuilder {
    entries: Vec<ClipboardEntry>,
}

impl ClipboardItemBuilder {
    /// Create an empty clipboard item builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a plain text entry.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.entries
            .push(ClipboardEntry::String(ClipboardString::new(text.into())));
        self
    }

    /// Add a text entry with raw metadata.
    pub fn text_with_metadata(
        mut self,
        text: impl Into<String>,
        metadata: impl Into<String>,
    ) -> Self {
        self.entries.push(ClipboardEntry::String(ClipboardString {
            text: text.into(),
            metadata: Some(metadata.into()),
        }));
        self
    }

    /// Add a text entry with JSON metadata.
    pub fn text_with_json_metadata<T: Serialize>(
        mut self,
        text: impl Into<String>,
        metadata: T,
    ) -> Self {
        self.entries.push(ClipboardEntry::String(
            ClipboardString::new(text.into()).with_json_metadata(metadata),
        ));
        self
    }

    /// Add a text entry with JSON metadata, reporting serialization errors.
    pub fn try_text_with_json_metadata<T: Serialize>(
        mut self,
        text: impl Into<String>,
        metadata: T,
    ) -> Result<Self> {
        self.entries.push(ClipboardEntry::String(
            ClipboardString::new(text.into()).try_with_json_metadata(metadata)?,
        ));
        Ok(self)
    }

    /// Add a rich HTML entry with a plain-text fallback.
    pub fn html(mut self, plain_text: impl Into<String>, html: impl Into<String>) -> Result<Self> {
        self.entries
            .push(ClipboardEntry::String(ClipboardString::from_html(
                plain_text.into(),
                html.into(),
            )?));
        Ok(self)
    }

    /// Add an image entry.
    pub fn image(mut self, image: Image) -> Self {
        self.entries.push(ClipboardEntry::Image(image));
        self
    }

    /// Add an image entry by cloning an existing image.
    pub fn image_ref(mut self, image: &Image) -> Self {
        self.entries.push(ClipboardEntry::Image(image.clone()));
        self
    }

    /// Return the configured entries.
    pub fn entries(&self) -> &[ClipboardEntry] {
        &self.entries
    }

    /// Number of configured entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Number of configured text entries.
    pub fn text_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, ClipboardEntry::String(_)))
            .count()
    }

    /// Number of configured image entries.
    pub fn image_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, ClipboardEntry::Image(_)))
            .count()
    }

    /// Number of configured string entries that carry metadata.
    pub fn metadata_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| match entry {
                ClipboardEntry::String(string) => string.metadata().is_some(),
                ClipboardEntry::Image(_) => false,
            })
            .count()
    }

    /// Total UTF-8 byte length of configured text entries.
    pub fn text_len_bytes(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                ClipboardEntry::String(string) => Some(string.text().len()),
                ClipboardEntry::Image(_) => None,
            })
            .sum()
    }

    /// Return whether a configured text entry carries HTML metadata.
    pub fn has_html(&self) -> bool {
        self.entries.iter().any(|entry| match entry {
            ClipboardEntry::String(string) => string.has_html(),
            ClipboardEntry::Image(_) => false,
        })
    }

    /// Return whether any configured text entry exists.
    pub fn has_text(&self) -> bool {
        self.text_count() > 0
    }

    /// Return whether any configured image entry exists.
    pub fn has_image(&self) -> bool {
        self.image_count() > 0
    }

    /// Returns a content-safe summary for logs and agent traces before writing.
    pub fn to_text(&self) -> String {
        format!(
            "clipboard item builder: {} entries, {} text, {} images, html {}, metadata {}, text {} bytes",
            self.entry_count(),
            self.text_count(),
            self.image_count(),
            self.has_html(),
            self.metadata_count(),
            self.text_len_bytes()
        )
    }

    /// Validate the builder configuration.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.entries.is_empty(),
            "clipboard item must contain at least one entry"
        );
        for entry in &self.entries {
            match entry {
                ClipboardEntry::String(string) => string.validate()?,
                ClipboardEntry::Image(image) => image.validate()?,
            }
        }
        Ok(())
    }

    /// Build the clipboard item.
    pub fn build(self) -> Result<ClipboardItem> {
        self.validate()?;
        Ok(ClipboardItem {
            entries: self.entries,
        })
    }

    /// Build the clipboard item using the checked-builder naming convention.
    pub fn build_checked(self) -> Result<ClipboardItem> {
        self.build()
    }
}

impl From<ClipboardString> for ClipboardEntry {
    fn from(value: ClipboardString) -> Self {
        Self::String(value)
    }
}

impl From<String> for ClipboardEntry {
    fn from(value: String) -> Self {
        Self::from(ClipboardString::from(value))
    }
}

impl From<Image> for ClipboardEntry {
    fn from(value: Image) -> Self {
        Self::Image(value)
    }
}

impl From<ClipboardEntry> for ClipboardItem {
    fn from(value: ClipboardEntry) -> Self {
        Self {
            entries: vec![value],
        }
    }
}

impl From<String> for ClipboardItem {
    fn from(value: String) -> Self {
        Self::from(ClipboardEntry::from(value))
    }
}

impl From<Image> for ClipboardItem {
    fn from(value: Image) -> Self {
        Self::from(ClipboardEntry::from(value))
    }
}

/// One of the editor's supported image formats (e.g. PNG, JPEG) - used when dealing with images in the clipboard
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumIter, Hash)]
pub enum ImageFormat {
    // Sorted from most to least likely to be pasted into an editor,
    // which matters when we iterate through them trying to see if
    // clipboard content matches them.
    /// .png
    Png,
    /// .jpeg or .jpg
    Jpeg,
    /// .webp
    Webp,
    /// .gif
    Gif,
    /// .svg
    Svg,
    /// .bmp
    Bmp,
    /// .tif or .tiff
    Tiff,
}

impl ImageFormat {
    /// Stable lowercase key for summaries and generated routing.
    pub const fn key(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Webp => "webp",
            ImageFormat::Gif => "gif",
            ImageFormat::Svg => "svg",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
        }
    }

    /// Returns the mime type for the ImageFormat
    pub const fn mime_type(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Webp => "image/webp",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Svg => "image/svg+xml",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::Tiff => "image/tiff",
        }
    }

    /// Returns the ImageFormat for the given mime type
    pub fn from_mime_type(mime_type: &str) -> Option<Self> {
        match mime_type {
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            "image/gif" => Some(Self::Gif),
            "image/svg+xml" => Some(Self::Svg),
            "image/bmp" => Some(Self::Bmp),
            "image/tiff" | "image/tif" => Some(Self::Tiff),
            _ => None,
        }
    }
}

/// An image, with a format and certain bytes
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// The image format the bytes represent (e.g. PNG)
    pub format: ImageFormat,
    /// The raw image bytes
    pub bytes: Vec<u8>,
    /// The unique ID for the image
    id: u64,
}

impl Hash for Image {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.id);
    }
}

impl Image {
    /// An empty image containing no data
    pub fn empty() -> Self {
        Self::from_bytes(ImageFormat::Png, Vec::new())
    }

    /// Create an image from a format and bytes
    pub fn from_bytes(format: ImageFormat, bytes: Vec<u8>) -> Self {
        Self {
            id: hash(&bytes),
            format,
            bytes,
        }
    }

    /// Get this image's ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Image format for these bytes.
    pub fn format(&self) -> ImageFormat {
        self.format
    }

    /// Raw encoded image bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Encoded byte length.
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether this image contains encoded bytes.
    pub fn has_bytes(&self) -> bool {
        !self.bytes.is_empty()
    }

    /// Content-safe summary for clipboard/native-image traces.
    pub fn to_text(&self) -> String {
        format!(
            "image: format {}, bytes {}, empty {}",
            self.format.key(),
            self.byte_len(),
            !self.has_bytes()
        )
    }

    /// Validate this image for clipboard use.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.bytes.is_empty(),
            "clipboard image bytes cannot be empty"
        );
        Ok(())
    }

    /// Use the GPUI `use_asset` API to make this image renderable
    pub fn use_render_image(
        self: Arc<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        ImageSource::Image(self)
            .use_data(None, window, cx)
            .and_then(|result| result.ok())
    }

    /// Use the GPUI `get_asset` API to make this image renderable
    pub fn get_render_image(
        self: Arc<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        ImageSource::Image(self)
            .get_data(None, window, cx)
            .and_then(|result| result.ok())
    }

    /// Use the GPUI `remove_asset` API to drop this image, if possible.
    pub fn remove_asset(self: Arc<Self>, cx: &mut App) {
        ImageSource::Image(self).remove_asset(cx);
    }

    /// Convert the clipboard image to an `ImageData` object.
    pub fn to_image_data(&self, svg_renderer: SvgRenderer) -> Result<Arc<RenderImage>> {
        fn frames_for_image(
            bytes: &[u8],
            format: image::ImageFormat,
        ) -> Result<SmallVec<[Frame; 1]>> {
            let mut data = image::load_from_memory_with_format(bytes, format)?.into_rgba8();

            // Convert from RGBA to BGRA.
            for pixel in data.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }

            Ok(SmallVec::from_elem(Frame::new(data), 1))
        }

        let frames = match self.format {
            ImageFormat::Gif => {
                let decoder = GifDecoder::new(Cursor::new(&self.bytes))?;
                let mut frames = SmallVec::new();

                for frame in decoder.into_frames() {
                    let mut frame = frame?;
                    // Convert from RGBA to BGRA.
                    for pixel in frame.buffer_mut().chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    frames.push(frame);
                }

                frames
            }
            ImageFormat::Png => frames_for_image(&self.bytes, image::ImageFormat::Png)?,
            ImageFormat::Jpeg => frames_for_image(&self.bytes, image::ImageFormat::Jpeg)?,
            ImageFormat::Webp => frames_for_image(&self.bytes, image::ImageFormat::WebP)?,
            ImageFormat::Bmp => frames_for_image(&self.bytes, image::ImageFormat::Bmp)?,
            ImageFormat::Tiff => frames_for_image(&self.bytes, image::ImageFormat::Tiff)?,
            ImageFormat::Svg => {
                let pixmap = svg_renderer.render_pixmap(&self.bytes, SvgSize::ScaleFactor(1.0))?;

                let buffer =
                    image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take())
                        .unwrap();

                SmallVec::from_elem(Frame::new(buffer), 1)
            }
        };

        Ok(Arc::new(RenderImage::new(frames)))
    }
}

/// A clipboard item that should be copied to the clipboard
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardString {
    pub(crate) text: String,
    pub(crate) metadata: Option<String>,
}

impl ClipboardString {
    /// Create a new clipboard string with the given text
    pub fn new(text: String) -> Self {
        Self {
            text,
            metadata: None,
        }
    }

    /// Create an HTML clipboard string with a plain-text fallback.
    pub fn from_html(text: String, html: String) -> Result<Self> {
        validate_clipboard_text(&text)?;
        let metadata = ClipboardHtmlMetadata::new(html)?;
        ClipboardString::new(text).try_with_json_metadata(metadata)
    }

    /// Return a new clipboard string with HTML metadata.
    pub fn with_html_metadata(mut self, html: impl Into<String>) -> Result<Self> {
        self.validate()?;
        let metadata = ClipboardHtmlMetadata::new(html)?;
        self.metadata = Some(serde_json::to_string(&metadata)?);
        Ok(self)
    }

    /// Return a new clipboard item with the metadata replaced by the given metadata,
    /// after serializing it as JSON.
    pub fn with_json_metadata<T: Serialize>(mut self, metadata: T) -> Self {
        self.metadata = Some(serde_json::to_string(&metadata).unwrap());
        self
    }

    /// Return a new clipboard item with JSON metadata, reporting serialization errors.
    pub fn try_with_json_metadata<T: Serialize>(mut self, metadata: T) -> Result<Self> {
        self.metadata = Some(serde_json::to_string(&metadata)?);
        Ok(self)
    }

    /// Get the text of the clipboard string
    pub fn text(&self) -> &String {
        &self.text
    }

    /// Get the raw metadata string, if present.
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    /// Return this string's HTML metadata, if present.
    pub fn html(&self) -> Option<String> {
        self.metadata_json::<ClipboardHtmlMetadata>()
            .and_then(|metadata| (metadata.kind == "html").then_some(metadata.html))
    }

    /// Return whether this string has HTML metadata.
    pub fn has_html(&self) -> bool {
        self.html().is_some()
    }

    /// Get the owned text of the clipboard string
    pub fn into_text(self) -> String {
        self.text
    }

    /// Get the metadata of the clipboard string, formatted as JSON
    pub fn metadata_json<T>(&self) -> Option<T>
    where
        T: for<'a> Deserialize<'a>,
    {
        self.metadata
            .as_ref()
            .and_then(|m| serde_json::from_str(m).ok())
    }

    /// Validate this string entry.
    pub fn validate(&self) -> Result<()> {
        validate_clipboard_text(&self.text)?;
        if let Some(metadata) = &self.metadata {
            validate_clipboard_metadata(metadata)?;
            if let Some(html) = self.html() {
                validate_clipboard_html(&html)?;
            }
        }
        Ok(())
    }

    #[cfg_attr(any(target_os = "linux", target_os = "freebsd"), allow(dead_code))]
    pub(crate) fn text_hash(text: &str) -> u64 {
        let mut hasher = SeaHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

fn validate_clipboard_text(text: &str) -> Result<()> {
    anyhow::ensure!(!text.is_empty(), "clipboard text cannot be empty");
    anyhow::ensure!(
        !text.contains('\0'),
        "clipboard text cannot contain NUL bytes"
    );
    Ok(())
}

fn validate_clipboard_metadata(metadata: &str) -> Result<()> {
    anyhow::ensure!(
        !metadata.trim().is_empty(),
        "clipboard metadata cannot be empty"
    );
    anyhow::ensure!(
        !metadata.contains('\0'),
        "clipboard metadata cannot contain NUL bytes"
    );
    Ok(())
}

fn validate_clipboard_html(html: &str) -> Result<()> {
    anyhow::ensure!(!html.trim().is_empty(), "clipboard HTML cannot be empty");
    anyhow::ensure!(
        !html.contains('\0'),
        "clipboard HTML cannot contain NUL bytes"
    );
    anyhow::ensure!(
        !html
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')),
        "clipboard HTML cannot contain control characters"
    );
    Ok(())
}

impl From<String> for ClipboardString {
    fn from(value: String) -> Self {
        Self {
            text: value,
            metadata: None,
        }
    }
}
