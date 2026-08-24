//! GTK4/GSK production window backend for native Linux WebKitGTK 6 hosts.
//!
//! GTK owns the native window, event loop and child-widget hierarchy. Kael
//! keeps ownership of retained scene state and application callbacks. Keeping
//! both the GSK paintable and WebKit widgets in one `GtkFixed` is the key
//! invariant: stacking, clipping, focus and destruction are compositor-native
//! instead of being approximated by detached overlay surfaces.

use super::{
    LinuxClient, LinuxCommon,
    accessibility::AtSpiAccessibleRoot,
    gtk4_scene::Gtk4SceneRenderer,
    gtk4_webview::{
        Gtk4WebViewHost, dispatch_webview_command, sync_webviews, webview_intercepts_pointer,
        webview_owns_focus,
    },
    open_uri_internal, reveal_path_internal,
};
use crate::{
    AnyWindowHandle, Bounds, Capslock, ClipboardItem, CursorStyle, DispatchEventResult, DisplayId,
    ExternalPaths, FileDropEvent, GameInputAvailability, GameInputCapabilities, GameInputError,
    GameInputErrorKind, GpuSpecs, Image, ImageFormat, KeyDownEvent, KeyUpEvent, Keystroke,
    LinuxKeyboardLayout, Modifiers, ModifiersChangedEvent, MouseButton, MouseMoveEvent,
    NavigationDirection, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput,
    PlatformInputHandler, PlatformKeyboardLayout, PlatformWebView, PlatformWebViewCommand, Point,
    PointerButtons, PointerId, PointerInputEvent, PointerLockStatus, PointerPhase, PointerType,
    PromptButton, PromptLevel, RequestFrameOptions, ResizeEdge, ScaledPixels, Scene, ScrollDelta,
    ScrollWheelEvent, SharedString, Size, TouchPhase, TrayMenuItem, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowDecorations, WindowKind,
    WindowParams, point, px, size,
};
use anyhow::Context as _;
use async_task::Runnable;
use calloop::{EventLoop, channel::Channel};
use collections::{FxHashMap, FxHashSet};
use futures::channel::oneshot;
use gdk4_wayland::prelude::WaylandSurfaceExtManual as _;
use gdk4_x11::prelude::*;
use gtk4::{Application, ApplicationWindow, Fixed, Picture, gdk, gio, glib, prelude::*};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    os::fd::AsRawFd as _,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr::NonNull,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use uuid::Uuid;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols::wp::{
    pointer_constraints::zv1::client::{zwp_locked_pointer_v1, zwp_pointer_constraints_v1},
    relative_pointer::zv1::client::{zwp_relative_pointer_manager_v1, zwp_relative_pointer_v1},
};
use x11rb::{
    connection::Connection as _,
    protocol::{
        Event as X11Event, xinput,
        xinput::ConnectionExt as _,
        xproto::{ConnectionExt as _, EventMask, GrabMode},
    },
    rust_connection::RustConnection,
};

const GTK_CLIPBOARD_TEXT_LIMIT: usize = 8 * 1024 * 1024;
const GTK_CLIPBOARD_HTML_LIMIT: usize = 16 * 1024 * 1024;
const GTK_CLIPBOARD_IMAGE_LIMIT: usize = 64 * 1024 * 1024;
const GTK_CLIPBOARD_METADATA_MIME: &str = "application/x-kael-clipboard-metadata+json";
static GTK4_EVENT_WAKEUPS: AtomicU64 = AtomicU64::new(0);
static GTK4_FRAME_TICKS: AtomicU64 = AtomicU64::new(0);
static NEXT_GTK4_POINTER_LOCK_TOKEN: AtomicU64 = AtomicU64::new(1);
const XINPUT_ALL_DEVICE_GROUPS: xinput::DeviceId = 1;
thread_local! {
    static GTK4_POINTER_LOCK_WINDOWS: RefCell<FxHashMap<u64, Weak<RefCell<Gtk4WindowState>>>> =
        RefCell::new(FxHashMap::default());
}

/// Number of times GLib has observed actual work on Kael's calloop bridge.
/// Used only by the native-Wayland release smoke to prove idle operation does
/// not fall back to a periodic timer.
#[doc(hidden)]
pub(crate) fn gtk4_wayland_event_wakeup_count() -> u64 {
    GTK4_EVENT_WAKEUPS.load(Ordering::Relaxed)
}

/// Number of GTK frame-clock callbacks observed by Kael windows.
///
/// The release smoke uses this together with the calloop counter to ensure a
/// mapped but idle window does not keep either event source hot.
#[doc(hidden)]
pub(crate) fn gtk4_wayland_frame_tick_count() -> u64 {
    GTK4_FRAME_TICKS.load(Ordering::Relaxed)
}

struct Gtk4ClientState {
    application: Application,
    common: LinuxCommon,
    event_loop: Rc<RefCell<EventLoop<'static, ()>>>,
    calloop_source_id: Option<u32>,
    windows: Vec<Weak<RefCell<Gtk4WindowState>>>,
    cursor_style: CursorStyle,
}

/// GTK-owned native Linux client selected by `webview-gtk4`.
#[derive(Clone)]
pub(crate) struct Gtk4Client(Rc<RefCell<Gtk4ClientState>>);

impl Gtk4Client {
    pub(crate) fn new() -> anyhow::Result<Self> {
        gtk4::init().context("initializing GTK4")?;
        let application = Application::builder()
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        application.connect_activate(|_| {});
        application
            .register(None::<&gio::Cancellable>)
            .context("registering the GTK4 application")?;

        // LinuxCommon needs a LoopSignal for its shared quit contract. GTK
        // owns dispatch for this backend, so the calloop remains dormant.
        let event_loop = Rc::new(RefCell::new(
            EventLoop::try_new().context("creating GTK4 event bridge")?,
        ));
        let (common, main_receiver, network_rx, system_power_rx) =
            LinuxCommon::new(event_loop.borrow().get_signal());
        let client = Self(Rc::new(RefCell::new(Gtk4ClientState {
            application,
            common,
            event_loop,
            calloop_source_id: None,
            windows: Vec::new(),
            cursor_style: CursorStyle::Arrow,
        })));
        client.install_dispatch_source(main_receiver, network_rx, system_power_rx)?;
        Ok(client)
    }

    fn install_dispatch_source(
        &self,
        main_receiver: Channel<Runnable>,
        network_rx: Channel<crate::NetworkStatus>,
        system_power_rx: Channel<crate::SystemPowerEvent>,
    ) -> anyhow::Result<()> {
        let event_loop = self.0.borrow().event_loop.clone();
        let handle = event_loop.borrow().handle();

        handle
            .insert_source(main_receiver, move |event, _, _| {
                if let calloop::channel::Event::Msg(runnable) = event {
                    super::catch_platform_callback("foreground task", (), || {
                        runnable.run();
                    });
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("registering GTK4 foreground-task source failed: {error:?}")
            })?;

        let weak = Rc::downgrade(&self.0);
        handle
            .insert_source(network_rx, move |event, _, _| {
                let calloop::channel::Event::Msg(status) = event else {
                    return;
                };
                let Some(state) = weak.upgrade() else { return };
                let callback = {
                    let mut state = state.borrow_mut();
                    let previous = state.common.last_network_status;
                    state.common.last_network_status = status;
                    (status != previous)
                        .then(|| state.common.callbacks.network_status_change.take())
                        .flatten()
                };
                if let Some(mut callback) = callback {
                    super::catch_platform_callback("network change", (), || callback(status));
                    state.borrow_mut().common.callbacks.network_status_change = Some(callback);
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("registering GTK4 network-status source failed: {error:?}")
            })?;

        let weak = Rc::downgrade(&self.0);
        handle
            .insert_source(system_power_rx, move |event, _, _| {
                let calloop::channel::Event::Msg(event) = event else {
                    return;
                };
                let Some(state) = weak.upgrade() else { return };
                let callback = state.borrow_mut().common.callbacks.system_power.take();
                if let Some(mut callback) = callback {
                    super::catch_platform_callback("system power", (), || callback(event));
                    state.borrow_mut().common.callbacks.system_power = Some(callback);
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("registering GTK4 system-power source failed: {error:?}")
            })?;

        let source_id = install_calloop_glib_source(event_loop);
        anyhow::ensure!(
            source_id != 0,
            "GLib rejected the GTK4 calloop event source"
        );
        self.0.borrow_mut().calloop_source_id = Some(source_id);
        Ok(())
    }

    fn live_windows(&self) -> Vec<Rc<RefCell<Gtk4WindowState>>> {
        let mut state = self.0.borrow_mut();
        let mut live = Vec::with_capacity(state.windows.len());
        state.windows.retain(|weak| {
            if let Some(window) = weak.upgrade() {
                live.push(window);
                true
            } else {
                false
            }
        });
        live
    }
}

impl Drop for Gtk4ClientState {
    fn drop(&mut self) {
        if let Some(source_id) = self.calloop_source_id.take() {
            // The source was installed on the GTK main thread and deliberately
            // never removes itself, so this is the single matching removal.
            unsafe {
                glib::ffi::g_source_remove(source_id);
            }
        }
    }
}

struct CalloopGlibSource {
    event_loop: Rc<RefCell<EventLoop<'static, ()>>>,
}

unsafe extern "C" fn dispatch_calloop_glib_source(
    _fd: i32,
    _condition: glib::ffi::GIOCondition,
    user_data: *mut c_void,
) -> glib::ffi::gboolean {
    GTK4_EVENT_WAKEUPS.fetch_add(1, Ordering::Relaxed);
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `user_data` is allocated by `install_calloop_glib_source`
        // and remains owned by GLib until the matching destroy notifier.
        let source = unsafe { &*(user_data.cast::<CalloopGlibSource>()) };
        let Ok(mut event_loop) = source.event_loop.try_borrow_mut() else {
            log::warn!("skipping a re-entrant GTK4 calloop dispatch");
            return;
        };
        if let Err(error) = event_loop.dispatch(Duration::ZERO, &mut ()) {
            log::error!("dispatching GTK4 calloop source failed: {error:?}");
        }
    }));
    if result.is_err() {
        log::error!("panic while dispatching GTK4 calloop source was contained");
    }
    1
}

unsafe extern "C" fn destroy_calloop_glib_source(user_data: *mut c_void) {
    // SAFETY: GLib invokes this exactly once for the pointer transferred in
    // `install_calloop_glib_source`.
    unsafe {
        drop(Box::from_raw(user_data.cast::<CalloopGlibSource>()));
    }
}

unsafe extern "C" {
    fn g_unix_fd_add_full(
        priority: i32,
        fd: i32,
        condition: glib::ffi::GIOCondition,
        function: Option<
            unsafe extern "C" fn(i32, glib::ffi::GIOCondition, *mut c_void) -> glib::ffi::gboolean,
        >,
        user_data: *mut c_void,
        notify: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> u32;
}

fn install_calloop_glib_source(event_loop: Rc<RefCell<EventLoop<'static, ()>>>) -> u32 {
    let fd = event_loop.borrow().as_raw_fd();
    let source = Box::new(CalloopGlibSource { event_loop });
    // SAFETY: `fd` remains owned by the retained event loop, the callback ABI
    // matches `GUnixFDSourceFunc`, and ownership of `source` is transferred to
    // GLib with a matching destroy notifier.
    unsafe {
        g_unix_fd_add_full(
            glib::ffi::G_PRIORITY_DEFAULT,
            fd,
            glib::ffi::G_IO_IN | glib::ffi::G_IO_ERR | glib::ffi::G_IO_HUP,
            Some(dispatch_calloop_glib_source),
            Box::into_raw(source).cast(),
            Some(destroy_calloop_glib_source),
        )
    }
}

impl LinuxClient for Gtk4Client {
    fn compositor_name(&self) -> &'static str {
        gtk4_backend_name()
    }

    fn with_common<R>(&self, f: impl FnOnce(&mut LinuxCommon) -> R) -> R {
        f(&mut self.0.borrow_mut().common)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(LinuxKeyboardLayout::new("unknown".into()))
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        gtk_displays()
    }

    fn display(&self, id: DisplayId) -> Option<Rc<dyn PlatformDisplay>> {
        gtk_displays()
            .into_iter()
            .find(|display| display.id() == id)
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        gtk_displays().into_iter().next()
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        true
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<anyhow::Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>> {
        crate::platform::scap_screen_capture::start_scap_default_target_source(
            &self.0.borrow().common.foreground_executor,
        )
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> anyhow::Result<Box<dyn crate::PlatformWindow>> {
        let parent = params.parent.and_then(|parent_handle| {
            self.live_windows().into_iter().find_map(|state| {
                let state = state.borrow();
                (state.handle == parent_handle).then(|| state.window.clone())
            })
        });
        let appearance = self.0.borrow().common.appearance;
        let window = Gtk4Window::new(self, handle, params, appearance, parent.as_ref())?;
        self.0.borrow_mut().windows.push(Rc::downgrade(&window.0));
        Ok(Box::new(window))
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        if self.0.borrow().cursor_style == style {
            return;
        }
        self.0.borrow_mut().cursor_style = style;
        let name = cursor_name(style);
        for window in self.live_windows() {
            let mut state = window.borrow_mut();
            state.cursor_style = style;
            if state.pointer_lock.status() != PointerLockStatus::Locked {
                state.fixed.set_cursor_from_name(name);
            }
        }
    }

    fn open_uri(&self, uri: &str) {
        open_uri_internal(
            self.0.borrow().common.background_executor.clone(),
            uri,
            None,
        );
    }

    fn reveal_path(&self, path: PathBuf) {
        reveal_path_internal(
            self.0.borrow().common.background_executor.clone(),
            path,
            None,
        );
    }

    fn write_to_primary(&self, item: ClipboardItem) {
        write_clipboard(true, item);
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        write_clipboard(false, item);
    }

    fn clear_clipboard(&self) {
        if let Some(display) = gdk::Display::default() {
            let _ = display
                .clipboard()
                .set_content(None::<&gdk::ContentProvider>);
        }
    }

    fn read_from_primary(&self) -> Option<ClipboardItem> {
        read_clipboard(true)
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        read_clipboard(false)
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.live_windows().into_iter().find_map(|window| {
            let state = window.borrow();
            state.window.is_active().then_some(state.handle)
        })
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        Some(
            self.live_windows()
                .into_iter()
                .filter_map(|window| {
                    let state = window.borrow();
                    state.window.is_visible().then_some(state.handle)
                })
                .collect(),
        )
    }

    fn show_context_menu(
        &self,
        position: Point<Pixels>,
        items: Vec<TrayMenuItem>,
        callback: Box<dyn FnMut(SharedString)>,
    ) {
        if items.is_empty() {
            return;
        }
        let windows = self.live_windows();
        let target = windows
            .iter()
            .find(|window| window.borrow().window.is_active())
            .or_else(|| {
                windows
                    .iter()
                    .find(|window| window.borrow().window.is_visible())
            });
        let Some(target) = target else {
            log::warn!("GTK4 context menu requested without a visible Kael window");
            return;
        };
        show_gtk4_context_menu(target, position, items, callback);
    }

    fn run(&self) {
        let application = self.0.borrow().application.clone();
        let _ = application.run();
    }

    fn quit(&self) {
        let application = {
            let mut state = self.0.borrow_mut();
            state.common.signal.stop();
            state.application.clone()
        };
        application.quit();
    }
}

#[derive(Debug, Clone)]
struct Gtk4Display {
    id: DisplayId,
    uuid: Uuid,
    bounds: Bounds<Pixels>,
    refresh_rate: Option<f32>,
    scale_factor: f32,
}

impl Gtk4Display {
    fn from_monitor(index: u32, monitor: &gdk::Monitor) -> Self {
        let geometry = monitor.geometry();
        let connector = monitor
            .connector()
            .or_else(|| monitor.description())
            .map_or_else(|| format!("monitor-{index}"), |value| value.to_string());
        let mut identity = connector.into_bytes();
        identity.extend_from_slice(&geometry.x().to_ne_bytes());
        identity.extend_from_slice(&geometry.y().to_ne_bytes());
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, &identity);
        let uuid_bytes = uuid.as_bytes();
        let id = DisplayId(u32::from_ne_bytes([
            uuid_bytes[0],
            uuid_bytes[1],
            uuid_bytes[2],
            uuid_bytes[3],
        ]));
        let scale_factor = monitor.scale() as f32;
        Self {
            id,
            uuid,
            bounds: Bounds::new(
                Point {
                    x: px(geometry.x() as f32),
                    y: px(geometry.y() as f32),
                },
                Size {
                    width: px(geometry.width().max(0) as f32),
                    height: px(geometry.height().max(0) as f32),
                },
            ),
            refresh_rate: (monitor.refresh_rate() > 0)
                .then(|| monitor.refresh_rate() as f32 / 1000.0),
            scale_factor: valid_scale(scale_factor),
        }
    }
}

impl PlatformDisplay for Gtk4Display {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn uuid(&self) -> anyhow::Result<Uuid> {
        Ok(self.uuid)
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    fn refresh_rate(&self) -> Option<f32> {
        self.refresh_rate
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
}

fn gtk_monitors() -> Vec<gdk::Monitor> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index)?.downcast::<gdk::Monitor>().ok())
        .collect()
}

fn gtk_displays() -> Vec<Rc<dyn PlatformDisplay>> {
    gtk_monitors()
        .iter()
        .enumerate()
        .map(|(index, monitor)| {
            Rc::new(Gtk4Display::from_monitor(index as u32, monitor)) as Rc<dyn PlatformDisplay>
        })
        .collect()
}

fn gtk4_backend_name() -> &'static str {
    let Some(display) = gdk::Display::default() else {
        return "gtk4-unavailable";
    };
    if display.is::<gdk4_wayland::WaylandDisplay>() {
        "wayland-gtk4"
    } else if display.is::<gdk4_x11::X11Display>() {
        "x11-gtk4"
    } else {
        "gtk4-unknown"
    }
}

fn monitor_for_window(window: &ApplicationWindow) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let surface = window.surface()?;
    display.monitor_at_surface(&surface)
}

fn display_for_window(window: &ApplicationWindow) -> Option<Rc<dyn PlatformDisplay>> {
    let monitor = monitor_for_window(window)?;
    let index = gtk_monitors()
        .iter()
        .position(|candidate| candidate == &monitor)
        .unwrap_or(0);
    Some(Rc::new(Gtk4Display::from_monitor(index as u32, &monitor)))
}

#[derive(Default)]
struct Gtk4WindowCallbacks {
    should_close: Option<Box<dyn FnMut() -> bool>>,
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active: Option<Box<dyn FnMut(bool)>>,
    hovered: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    hit_test_window_control: Option<Box<dyn FnMut() -> Option<WindowControlArea>>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance: Option<Box<dyn FnMut()>>,
}

#[derive(Clone)]
struct Gtk4PointerLockTarget {
    token: u64,
    generation: u64,
}

impl Gtk4PointerLockTarget {
    fn window(&self) -> Option<Rc<RefCell<Gtk4WindowState>>> {
        GTK4_POINTER_LOCK_WINDOWS
            .with(|windows| windows.borrow().get(&self.token).and_then(Weak::upgrade))
    }
}

struct Gtk4RelativeMotion {
    target: Gtk4PointerLockTarget,
    dx: f64,
    dy: f64,
    timestamp_ms: f64,
    device_pixels: bool,
}

#[derive(Default)]
struct Gtk4PointerLockDispatch {
    pending_motion: Vec<Gtk4RelativeMotion>,
}

struct Gtk4WaylandPointerLockBackend {
    connection: Connection,
    event_queue: EventQueue<Gtk4PointerLockDispatch>,
    dispatch: Gtk4PointerLockDispatch,
    constraints: zwp_pointer_constraints_v1::ZwpPointerConstraintsV1,
    relative_manager: zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    locked_pointer: Option<zwp_locked_pointer_v1::ZwpLockedPointerV1>,
    relative_pointer: Option<zwp_relative_pointer_v1::ZwpRelativePointerV1>,
}

impl Gtk4WaylandPointerLockBackend {
    fn new() -> anyhow::Result<Self> {
        let display = gdk::Display::default()
            .context("GTK4 Wayland display is unavailable")?
            .downcast::<gdk4_wayland::WaylandDisplay>()
            .map_err(|_| anyhow::anyhow!("GTK4 display is not a native Wayland display"))?;
        let display = display
            .wl_display()
            .context("GTK4 did not expose its Wayland client connection")?;
        let backend = display
            .backend()
            .upgrade()
            .context("GTK4 Wayland display backend disappeared")?;
        let connection = Connection::from_backend(backend);
        let (globals, event_queue) = registry_queue_init::<Gtk4PointerLockDispatch>(&connection)
            .context("discovering GTK4 Wayland pointer-lock globals")?;
        let queue_handle = event_queue.handle();
        let constraints = globals
            .bind(&queue_handle, 1..=1, ())
            .context("Wayland compositor does not advertise pointer-constraints-v1")?;
        let relative_manager = globals
            .bind(&queue_handle, 1..=1, ())
            .context("Wayland compositor does not advertise relative-pointer-v1")?;
        Ok(Self {
            connection,
            event_queue,
            dispatch: Gtk4PointerLockDispatch::default(),
            constraints,
            relative_manager,
            locked_pointer: None,
            relative_pointer: None,
        })
    }

    fn dispatch_pending(&mut self) -> Vec<Gtk4RelativeMotion> {
        if let Err(error) = self.event_queue.dispatch_pending(&mut self.dispatch) {
            log::warn!("dispatching GTK4 pointer-lock events failed: {error:#}");
        }
        std::mem::take(&mut self.dispatch.pending_motion)
    }

    fn clear_request(&mut self) {
        if let Some(relative_pointer) = self.relative_pointer.take() {
            relative_pointer.destroy();
        }
        if let Some(locked_pointer) = self.locked_pointer.take() {
            locked_pointer.destroy();
        }
        self.connection.flush().ok();
    }
}

struct Gtk4X11PointerLockBackend {
    connection: RustConnection,
    root: u32,
    locked_window: Option<u32>,
    saved_root_position: Option<(i16, i16)>,
    target: Option<Gtk4PointerLockTarget>,
}

impl Gtk4X11PointerLockBackend {
    fn new() -> anyhow::Result<Self> {
        let display = gdk::Display::default()
            .context("GTK4 X11 display is unavailable")?
            .downcast::<gdk4_x11::X11Display>()
            .map_err(|_| anyhow::anyhow!("GTK4 display is not a native X11 display"))?;
        let (connection, screen_index) =
            x11rb::connect(None).context("connecting to the GTK4 X11 display")?;
        let root = connection
            .setup()
            .roots
            .get(screen_index)
            .context("the GTK4 X11 screen is unavailable")?
            .root;

        // Validate that both libraries resolved the same X screen. XIDs are
        // server-global, but a mismatched DISPLAY would make a later grab
        // fail in a much less actionable way.
        anyhow::ensure!(
            display.screen().screen_number() == screen_index as i32,
            "GTK4 and x11rb selected different X11 screens"
        );
        Ok(Self {
            connection,
            root,
            locked_window: None,
            saved_root_position: None,
            target: None,
        })
    }

    fn request(&mut self, window: u32, target: Gtk4PointerLockTarget) -> anyhow::Result<()> {
        if self.locked_window == Some(window) {
            self.target = Some(target);
            return Ok(());
        }
        anyhow::ensure!(
            self.locked_window.is_none(),
            "another GTK4 window currently owns the X11 pointer lock"
        );

        let saved_root_position = if super::x11_pointer_position_restore_is_safe() {
            let pointer = self
                .connection
                .query_pointer(window)
                .context("sending the X11 pointer-position query")?
                .reply()
                .context("saving the X11 pointer position")?;
            Some((pointer.root_x, pointer.root_y))
        } else {
            None
        };

        self.connection
            .xinput_xi_select_events(
                self.root,
                &[xinput::EventMask {
                    deviceid: XINPUT_ALL_DEVICE_GROUPS,
                    mask: vec![xinput::XIEventMask::RAW_MOTION],
                }],
            )
            .context("selecting XI2 raw motion")?
            .check()
            .context("the X server rejected XI2 raw motion")?;

        let reply = self
            .connection
            .grab_pointer(
                true,
                window,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                window,
                x11rb::NONE,
                x11rb::CURRENT_TIME,
            )
            .context("sending the X11 pointer-grab request")?
            .reply()
            .context("receiving the X11 pointer-grab reply")?;
        if reply.status != x11rb::protocol::xproto::GrabStatus::SUCCESS {
            let _ = self.clear_raw_motion_selection();
            anyhow::bail!(
                "the X server rejected pointer confinement: {:?}",
                reply.status
            );
        }

        if let Err(error) = self.connection.flush() {
            let _ = self.connection.ungrab_pointer(x11rb::CURRENT_TIME);
            let _ = self.clear_raw_motion_selection();
            return Err(error).context("flushing the X11 pointer-lock requests");
        }
        self.locked_window = Some(window);
        self.saved_root_position = saved_root_position;
        self.target = Some(target);
        Ok(())
    }

    fn dispatch_pending(&mut self) -> Vec<Gtk4RelativeMotion> {
        let mut motions = Vec::new();
        loop {
            let event = match self.connection.poll_for_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(error) => {
                    log::warn!("reading GTK4 X11 pointer-lock events failed: {error}");
                    break;
                }
            };
            let X11Event::XinputRawMotion(event) = event else {
                continue;
            };
            let Some(target) = self.target.clone() else {
                continue;
            };
            let dx = xinput_raw_axis(&event, 0);
            let dy = xinput_raw_axis(&event, 1);
            if dx.is_finite() && dy.is_finite() && (dx != 0.0 || dy != 0.0) {
                motions.push(Gtk4RelativeMotion {
                    target,
                    dx,
                    dy,
                    timestamp_ms: f64::from(event.time),
                    device_pixels: true,
                });
            }
        }
        motions
    }

    fn clear_request(&mut self) -> anyhow::Result<()> {
        if self.locked_window.is_none() {
            self.target = None;
            return Ok(());
        }
        let mut first_error = None;
        let ungrab = match self.connection.ungrab_pointer(x11rb::CURRENT_TIME) {
            Ok(cookie) => cookie.check().map_err(anyhow::Error::from),
            Err(error) => Err(anyhow::Error::from(error)),
        };
        if let Err(error) = ungrab {
            first_error = Some(anyhow::anyhow!("releasing the X11 pointer grab: {error}"));
        }
        if let Some((x, y)) = self.saved_root_position {
            let restore =
                match self
                    .connection
                    .warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, x, y)
                {
                    Ok(cookie) => cookie.check().map_err(anyhow::Error::from),
                    Err(error) => Err(anyhow::Error::from(error)),
                };
            if let Err(error) = restore
                && first_error.is_none()
            {
                first_error = Some(anyhow::anyhow!(
                    "restoring the X11 pointer position: {error}"
                ));
            }
        }
        if let Err(error) = self.clear_raw_motion_selection()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Err(error) = self.connection.flush()
            && first_error.is_none()
        {
            first_error = Some(anyhow::anyhow!("flushing X11 pointer unlock: {error}"));
        }
        self.locked_window = None;
        self.saved_root_position = None;
        self.target = None;
        first_error.map_or(Ok(()), Err)
    }

    fn clear_raw_motion_selection(&self) -> anyhow::Result<()> {
        self.connection
            .xinput_xi_select_events(
                self.root,
                &[xinput::EventMask {
                    deviceid: XINPUT_ALL_DEVICE_GROUPS,
                    mask: Vec::new(),
                }],
            )
            .context("clearing XI2 raw-motion selection")?
            .check()
            .context("the X server rejected the XI2 cleanup")
    }
}

impl Drop for Gtk4X11PointerLockBackend {
    fn drop(&mut self) {
        if let Err(error) = self.clear_request() {
            log::warn!("cleaning up GTK4 X11 pointer lock failed: {error:#}");
        }
    }
}

enum Gtk4PointerLockBackend {
    Wayland(Gtk4WaylandPointerLockBackend),
    X11(Gtk4X11PointerLockBackend),
}

impl Gtk4PointerLockBackend {
    fn new() -> anyhow::Result<Self> {
        let display = gdk::Display::default().context("GTK4 display is unavailable")?;
        if display.is::<gdk4_wayland::WaylandDisplay>() {
            Gtk4WaylandPointerLockBackend::new().map(Self::Wayland)
        } else if display.is::<gdk4_x11::X11Display>() {
            Gtk4X11PointerLockBackend::new().map(Self::X11)
        } else {
            anyhow::bail!("GTK4 is not using a supported Wayland or X11 display")
        }
    }

    fn dispatch_pending(&mut self) -> Vec<Gtk4RelativeMotion> {
        match self {
            Self::Wayland(backend) => backend.dispatch_pending(),
            Self::X11(backend) => backend.dispatch_pending(),
        }
    }

    fn clear_request(&mut self) -> anyhow::Result<()> {
        match self {
            Self::Wayland(backend) => {
                backend.clear_request();
                Ok(())
            }
            Self::X11(backend) => backend.clear_request(),
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Gtk4PointerLockDispatch {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(Gtk4PointerLockDispatch: ignore zwp_pointer_constraints_v1::ZwpPointerConstraintsV1);
delegate_noop!(Gtk4PointerLockDispatch: ignore zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1);

impl Dispatch<zwp_locked_pointer_v1::ZwpLockedPointerV1, Gtk4PointerLockTarget>
    for Gtk4PointerLockDispatch
{
    fn event(
        _: &mut Self,
        _: &zwp_locked_pointer_v1::ZwpLockedPointerV1,
        event: zwp_locked_pointer_v1::Event,
        target: &Gtk4PointerLockTarget,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(window) = target.window() else {
            return;
        };
        let mut state = window.borrow_mut();
        if state.pointer_lock_generation != target.generation {
            return;
        }
        match event {
            zwp_locked_pointer_v1::Event::Locked => {
                state.pointer_lock.lock();
                state
                    .fixed
                    .set_cursor_from_name(cursor_name(CursorStyle::None));
            }
            zwp_locked_pointer_v1::Event::Unlocked => {
                state.pointer_lock.unlock();
                state
                    .fixed
                    .set_cursor_from_name(cursor_name(state.cursor_style));
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_relative_pointer_v1::ZwpRelativePointerV1, Gtk4PointerLockTarget>
    for Gtk4PointerLockDispatch
{
    fn event(
        dispatch: &mut Self,
        _: &zwp_relative_pointer_v1::ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        target: &Gtk4PointerLockTarget,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zwp_relative_pointer_v1::Event::RelativeMotion {
            utime_hi,
            utime_lo,
            dx_unaccel,
            dy_unaccel,
            ..
        } = event
        else {
            return;
        };
        if dx_unaccel.is_finite() && dy_unaccel.is_finite() {
            dispatch.pending_motion.push(Gtk4RelativeMotion {
                target: target.clone(),
                dx: dx_unaccel,
                dy: dy_unaccel,
                timestamp_ms: (((u64::from(utime_hi) << 32) | u64::from(utime_lo)) as f64)
                    / 1_000.0,
                device_pixels: false,
            });
        }
    }
}

struct Gtk4WindowState {
    window: ApplicationWindow,
    fixed: Fixed,
    scene_picture: Picture,
    handle: AnyWindowHandle,
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    monitor_id: Option<DisplayId>,
    appearance: WindowAppearance,
    background: WindowBackgroundAppearance,
    hovered: bool,
    mouse_position: Point<Pixels>,
    modifiers: Modifiers,
    capslock: Capslock,
    pointer_buttons: PointerButtons,
    pointer_lock: crate::game_input::NativePointerLockState,
    pointer_lock_backend: Option<Rc<RefCell<Gtk4PointerLockBackend>>>,
    pointer_lock_token: u64,
    pointer_lock_generation: u64,
    cursor_style: CursorStyle,
    last_pointer_event: Option<gdk::Event>,
    pressed_keys: FxHashSet<u32>,
    touches: FxHashMap<gdk::EventSequence, Gtk4TouchState>,
    next_touch_id: i64,
    drag_active: bool,
    mouse_passthrough: bool,
    app_id: Option<String>,
    decorations: WindowDecorations,
    input_handler: Option<PlatformInputHandler>,
    ime_context: gtk4::IMMulticontext,
    callbacks: Gtk4WindowCallbacks,
    renderer: Gtk4SceneRenderer,
    webviews: FxHashMap<SharedString, Gtk4WebViewHost>,
    context_menu: Option<gtk4::Popover>,
    fullscreen: bool,
    frame_polling: bool,
    frame_tick_id: Option<gtk4::TickCallbackId>,
    metric_dispatch_scheduled: bool,
    pending_resize: bool,
    pending_moved: bool,
    pending_metric_frame: bool,
    active_dispatch_scheduled: bool,
    pending_active: Option<bool>,
    accessibility_root: AtSpiAccessibleRoot,
}

#[derive(Clone, Copy)]
struct Gtk4TouchState {
    id: PointerId,
    position: Point<Pixels>,
    is_primary: bool,
}

/// Kael platform window backed by a GTK4 `ApplicationWindow`.
pub(crate) struct Gtk4Window(Rc<RefCell<Gtk4WindowState>>);

impl Gtk4Window {
    fn new(
        client: &Gtk4Client,
        handle: AnyWindowHandle,
        params: WindowParams,
        appearance: WindowAppearance,
        parent: Option<&ApplicationWindow>,
    ) -> anyhow::Result<Self> {
        let application = client.0.borrow().application.clone();
        let fixed = Fixed::new();
        fixed.set_hexpand(true);
        fixed.set_vexpand(true);
        fixed.set_focusable(true);
        fixed.set_cursor_from_name(cursor_name(client.0.borrow().cursor_style));

        let scene_picture = Picture::new();
        scene_picture.set_can_target(false);
        scene_picture.set_content_fit(gtk4::ContentFit::Fill);
        scene_picture.set_hexpand(false);
        scene_picture.set_vexpand(false);
        fixed.put(&scene_picture, 0.0, 0.0);

        let ime_context = gtk4::IMMulticontext::new();
        ime_context.set_client_widget(Some(&fixed));

        let width = finite_extent(params.bounds.size.width.0);
        let height = finite_extent(params.bounds.size.height.0);
        let window = ApplicationWindow::builder()
            .application(&application)
            .default_width(width)
            .default_height(height)
            .resizable(params.is_resizable)
            .child(&fixed)
            .build();
        if let Some(parent) = parent {
            window.set_transient_for(Some(parent));
            window.set_modal(params.kind == WindowKind::Floating);
        }
        if let Some(title) = params.titlebar.and_then(|titlebar| titlebar.title) {
            window.set_title(Some(&title));
        }
        if let Some(minimum) = params.window_min_size {
            fixed.set_size_request(
                finite_extent(minimum.width.0),
                finite_extent(minimum.height.0),
            );
        }

        let initial_display = display_for_window(&window);
        let scale_factor = initial_display
            .as_ref()
            .map_or(1.0, |display| display.scale_factor());
        let monitor_id = initial_display.as_ref().map(|display| display.id());
        scene_picture.set_size_request(width, height);

        let cursor_style = client.0.borrow().cursor_style;
        let pointer_lock_token = NEXT_GTK4_POINTER_LOCK_TOKEN.fetch_add(1, Ordering::Relaxed);
        let this = Self(Rc::new(RefCell::new(Gtk4WindowState {
            window: window.clone(),
            fixed: fixed.clone(),
            scene_picture,
            handle,
            bounds: params.bounds,
            scale_factor,
            monitor_id,
            appearance,
            background: WindowBackgroundAppearance::Opaque,
            hovered: false,
            mouse_position: Point::default(),
            modifiers: Modifiers::default(),
            capslock: Capslock::default(),
            pointer_buttons: PointerButtons::empty(),
            pointer_lock: crate::game_input::NativePointerLockState::new(true),
            pointer_lock_backend: None,
            pointer_lock_token,
            pointer_lock_generation: 0,
            cursor_style,
            last_pointer_event: None,
            pressed_keys: FxHashSet::default(),
            touches: FxHashMap::default(),
            next_touch_id: 2,
            drag_active: false,
            mouse_passthrough: params.mouse_passthrough,
            app_id: None,
            decorations: WindowDecorations::Server,
            input_handler: None,
            ime_context,
            callbacks: Gtk4WindowCallbacks::default(),
            renderer: Gtk4SceneRenderer::default(),
            webviews: FxHashMap::default(),
            context_menu: None,
            fullscreen: false,
            frame_polling: false,
            frame_tick_id: None,
            metric_dispatch_scheduled: false,
            pending_resize: false,
            pending_moved: false,
            pending_metric_frame: false,
            active_dispatch_scheduled: false,
            pending_active: None,
            accessibility_root: AtSpiAccessibleRoot::new(),
        })));
        GTK4_POINTER_LOCK_WINDOWS.with(|windows| {
            windows
                .borrow_mut()
                .insert(pointer_lock_token, Rc::downgrade(&this.0));
        });
        this.connect_window_signals();
        this.install_input_controllers();
        if params.show {
            if params.focus {
                window.present();
            } else {
                window.set_visible(true);
            }
        }
        apply_mouse_passthrough(&this.0);
        Ok(this)
    }

    fn connect_window_signals(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.borrow().window.connect_realize(move |_| {
            let Some(state) = weak.upgrade() else { return };
            apply_app_id(&state);
            connect_surface_metric_signals(&state);
            let fixed = state.borrow().fixed.clone();
            update_window_metrics(&state, &fixed);
        });

        let weak = Rc::downgrade(&self.0);
        self.0.borrow().window.connect_close_request(move |_| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let callback = state.borrow_mut().callbacks.should_close.take();
            let should_close = callback.map_or(true, |mut callback| {
                let result =
                    super::catch_platform_callback("window should close", true, &mut callback);
                state.borrow_mut().callbacks.should_close = Some(callback);
                result
            });
            if should_close {
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            }
        });

        let weak = Rc::downgrade(&self.0);
        self.0.borrow().window.connect_destroy(move |_| {
            let Some(state) = weak.upgrade() else { return };
            dismiss_gtk4_context_menu(&state);
            release_gtk_pointer_lock(&state).ok();
            if let Some(callback) = state.borrow_mut().callbacks.close.take() {
                super::catch_platform_callback("window closed", (), callback);
            }
        });

        let weak = Rc::downgrade(&self.0);
        self.0.borrow().window.connect_map(move |_| {
            if let Some(state) = weak.upgrade() {
                apply_mouse_passthrough(&state);
            }
        });

        let weak = Rc::downgrade(&self.0);
        self.0
            .borrow()
            .window
            .connect_is_active_notify(move |window| {
                let Some(state) = weak.upgrade() else { return };
                if !window.is_active()
                    && state.borrow().pointer_lock.status() != PointerLockStatus::Unlocked
                {
                    release_gtk_pointer_lock(&state).ok();
                }
                defer_window_active(&state, window.is_active());
            });
    }

    fn install_input_controllers(&self) {
        let fixed = self.0.borrow().fixed.clone();

        let motion = gtk4::EventControllerMotion::new();
        motion.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        let weak = Rc::downgrade(&self.0);
        motion.connect_enter(move |controller, x, y| {
            let Some(state) = weak.upgrade() else { return };
            if event_is_pointer_emulated(controller) {
                return;
            }
            set_window_hovered(&state, true);
            dispatch_mouse_pointer(&state, controller, PointerPhase::Enter, x, y, None, 0);
        });
        let weak = Rc::downgrade(&self.0);
        motion.connect_motion(move |controller, x, y| {
            let Some(state) = weak.upgrade() else { return };
            if event_is_pointer_emulated(controller) {
                return;
            }
            if state.borrow().pointer_lock.status() == PointerLockStatus::Locked {
                return;
            }
            dispatch_mouse_pointer(&state, controller, PointerPhase::Move, x, y, None, 0);
        });
        let weak = Rc::downgrade(&self.0);
        motion.connect_leave(move |controller| {
            let Some(state) = weak.upgrade() else { return };
            if event_is_pointer_emulated(controller) {
                return;
            }
            let position = state.borrow().mouse_position;
            dispatch_mouse_pointer(
                &state,
                controller,
                PointerPhase::Leave,
                f64::from(position.x.0),
                f64::from(position.y.0),
                None,
                0,
            );
            set_window_hovered(&state, false);
        });
        fixed.add_controller(motion);

        let click = gtk4::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        let weak = Rc::downgrade(&self.0);
        click.connect_pressed(move |gesture, count, x, y| {
            let Some(state) = weak.upgrade() else { return };
            if event_is_pointer_emulated(gesture) {
                return;
            }
            let position = point(px(x as f32), px(y as f32));
            let over_webview = {
                let state = state.borrow();
                webview_intercepts_pointer(&state.webviews, position)
            };
            if !over_webview {
                state.borrow().fixed.grab_focus();
            }
            dispatch_mouse_pointer(
                &state,
                gesture,
                PointerPhase::Down,
                x,
                y,
                mouse_button(gesture.current_button()),
                count.max(1) as usize,
            );
        });
        let weak = Rc::downgrade(&self.0);
        click.connect_released(move |gesture, count, x, y| {
            let Some(state) = weak.upgrade() else { return };
            if event_is_pointer_emulated(gesture) {
                return;
            }
            dispatch_mouse_pointer(
                &state,
                gesture,
                PointerPhase::Up,
                x,
                y,
                mouse_button(gesture.current_button()),
                count.max(1) as usize,
            );
        });
        fixed.add_controller(click);

        let scroll = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::BOTH_AXES | gtk4::EventControllerScrollFlags::KINETIC,
        );
        scroll.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        let weak = Rc::downgrade(&self.0);
        scroll.connect_scroll(move |controller, dx, dy| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let (position, modifiers, over_webview) = {
                let mut state = state.borrow_mut();
                let modifiers = modifiers_from_gdk(controller.current_event_state());
                state.modifiers = modifiers;
                (
                    state.mouse_position,
                    modifiers,
                    webview_intercepts_pointer(&state.webviews, state.mouse_position),
                )
            };
            if over_webview {
                return glib::Propagation::Proceed;
            }
            let delta = match controller.unit() {
                gdk::ScrollUnit::Surface => ScrollDelta::Pixels(point(
                    px(finite_input(-dx) as f32),
                    px(finite_input(-dy) as f32),
                )),
                _ => ScrollDelta::Lines(point(finite_input(-dx) as f32, finite_input(-dy) as f32)),
            };
            dispatch_window_input(
                &state,
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position,
                    delta,
                    modifiers,
                    touch_phase: TouchPhase::Moved,
                    is_momentum: false,
                }),
            )
        });
        fixed.add_controller(scroll);

        let key = gtk4::EventControllerKey::new();
        key.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        key.set_im_context(Some(&self.0.borrow().ime_context));
        let weak = Rc::downgrade(&self.0);
        key.connect_key_pressed(move |_, keyval, keycode, modifier_state| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if webview_focus_owned(&state) {
                return glib::Propagation::Proceed;
            }
            let modifiers = modifiers_from_gdk(modifier_state);
            let is_held = {
                let mut state = state.borrow_mut();
                state.modifiers = modifiers;
                !state.pressed_keys.insert(keycode)
            };
            dispatch_window_input(
                &state,
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: keystroke_from_gdk(keyval, modifiers),
                    is_held,
                }),
            )
        });
        let weak = Rc::downgrade(&self.0);
        key.connect_key_released(move |_, keyval, keycode, modifier_state| {
            let Some(state) = weak.upgrade() else { return };
            if webview_focus_owned(&state) {
                return;
            }
            let modifiers = modifiers_from_gdk(modifier_state);
            {
                let mut state = state.borrow_mut();
                state.modifiers = modifiers;
                state.pressed_keys.remove(&keycode);
            }
            let _ = dispatch_window_input(
                &state,
                PlatformInput::KeyUp(KeyUpEvent {
                    keystroke: keystroke_from_gdk(keyval, modifiers),
                }),
            );
        });
        let weak = Rc::downgrade(&self.0);
        key.connect_modifiers(move |_, modifier_state| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let modifiers = modifiers_from_gdk(modifier_state);
            let capslock = Capslock {
                on: modifier_state.contains(gdk::ModifierType::LOCK_MASK),
            };
            {
                let mut state = state.borrow_mut();
                state.modifiers = modifiers;
                state.capslock = capslock;
            }
            dispatch_window_input(
                &state,
                PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                    modifiers,
                    capslock,
                }),
            )
        });
        fixed.add_controller(key);

        let legacy = gtk4::EventControllerLegacy::new();
        legacy.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        let weak = Rc::downgrade(&self.0);
        legacy.connect_event(move |_, event| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            dispatch_touch_event(&state, event)
        });
        fixed.add_controller(legacy);

        let drop_target =
            gtk4::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        drop_target.set_preload(true);
        drop_target.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        let weak = Rc::downgrade(&self.0);
        drop_target.connect_enter(move |target, x, y| {
            let Some(state) = weak.upgrade() else {
                return gdk::DragAction::empty();
            };
            let position = safe_position(x, y);
            let over_webview = {
                let mut state = state.borrow_mut();
                state.mouse_position = position;
                webview_intercepts_pointer(&state.webviews, position)
            };
            if over_webview {
                return gdk::DragAction::empty();
            }
            if let Some(paths) = drop_target_paths(target)
                && !paths.is_empty()
            {
                state.borrow_mut().drag_active = true;
                let _ = dispatch_window_input(
                    &state,
                    PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }),
                );
            }
            gdk::DragAction::COPY
        });
        let weak = Rc::downgrade(&self.0);
        drop_target.connect_motion(move |_, x, y| {
            let Some(state) = weak.upgrade() else {
                return gdk::DragAction::empty();
            };
            let position = safe_position(x, y);
            if webview_intercepts_pointer(&state.borrow().webviews, position) {
                return gdk::DragAction::empty();
            }
            state.borrow_mut().mouse_position = position;
            let _ = dispatch_window_input(
                &state,
                PlatformInput::FileDrop(FileDropEvent::Pending { position }),
            );
            gdk::DragAction::COPY
        });
        let weak = Rc::downgrade(&self.0);
        drop_target.connect_leave(move |_| {
            let Some(state) = weak.upgrade() else { return };
            if state.borrow_mut().drag_active {
                state.borrow_mut().drag_active = false;
                let _ =
                    dispatch_window_input(&state, PlatformInput::FileDrop(FileDropEvent::Exited));
            }
        });
        let weak = Rc::downgrade(&self.0);
        drop_target.connect_drop(move |_, value, x, y| {
            let Some(state) = weak.upgrade() else {
                return false;
            };
            let position = safe_position(x, y);
            if webview_intercepts_pointer(&state.borrow().webviews, position) {
                return false;
            }
            if !state.borrow().drag_active {
                let paths = value
                    .get::<gdk::FileList>()
                    .ok()
                    .map(paths_from_file_list)
                    .unwrap_or_default();
                if paths.is_empty() {
                    return false;
                }
                let _ = dispatch_window_input(
                    &state,
                    PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }),
                );
            }
            state.borrow_mut().drag_active = false;
            let _ = dispatch_window_input(
                &state,
                PlatformInput::FileDrop(FileDropEvent::Submit { position }),
            );
            true
        });
        fixed.add_controller(drop_target);

        let ime = self.0.borrow().ime_context.clone();
        let weak = Rc::downgrade(&self.0);
        ime.connect_commit(move |_, text| {
            let Some(state) = weak.upgrade() else { return };
            if webview_focus_owned(&state) {
                return;
            }
            if text.len() == 1 {
                let _ = dispatch_window_input(
                    &state,
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke: Keystroke {
                            modifiers: Modifiers::default(),
                            key: text.to_string(),
                            key_char: Some(text.to_string()),
                        },
                        is_held: false,
                    }),
                );
            } else {
                with_input_handler(&state, |handler| {
                    handler.unmark_text();
                    handler.replace_text_in_range(None, text);
                });
            }
        });
        let weak = Rc::downgrade(&self.0);
        ime.connect_preedit_changed(move |ime| {
            let Some(state) = weak.upgrade() else { return };
            let (text, _, cursor_chars) = ime.preedit_string();
            let cursor_utf16 = text
                .chars()
                .take(cursor_chars.max(0) as usize)
                .map(char::len_utf16)
                .sum::<usize>();
            with_input_handler(&state, |handler| {
                if text.is_empty() {
                    handler.unmark_text();
                } else {
                    handler.replace_and_mark_text_in_range(
                        None,
                        &text,
                        Some(cursor_utf16..cursor_utf16),
                    );
                }
            });
        });
        let weak = Rc::downgrade(&self.0);
        ime.connect_preedit_end(move |_| {
            let Some(state) = weak.upgrade() else { return };
            with_input_handler(&state, PlatformInputHandler::unmark_text);
        });

        let ime = self.0.borrow().ime_context.clone();
        fixed.connect_has_focus_notify(move |fixed| {
            if fixed.has_focus() {
                ime.focus_in();
            } else {
                ime.focus_out();
            }
        });
    }
}

impl Drop for Gtk4Window {
    fn drop(&mut self) {
        dismiss_gtk4_context_menu(&self.0);
        release_gtk_pointer_lock(&self.0).ok();
        let token = self.0.borrow().pointer_lock_token;
        GTK4_POINTER_LOCK_WINDOWS.with(|windows| {
            windows.borrow_mut().remove(&token);
        });
    }
}

impl HasWindowHandle for Gtk4Window {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let surface = self
            .0
            .borrow()
            .window
            .surface()
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let raw = if let Ok(surface) = surface.clone().downcast::<gdk4_wayland::WaylandSurface>() {
            let surface = surface
                .wl_surface_raw()
                .ok_or(raw_window_handle::HandleError::Unavailable)?;
            raw_window_handle::RawWindowHandle::Wayland(
                raw_window_handle::WaylandWindowHandle::new(surface),
            )
        } else if let Ok(surface) = surface.downcast::<gdk4_x11::X11Surface>() {
            raw_window_handle::RawWindowHandle::Xlib(raw_window_handle::XlibWindowHandle::new(
                surface.xid(),
            ))
        } else {
            return Err(raw_window_handle::HandleError::Unavailable);
        };
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Gtk4Window {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let display = gdk::Display::default().ok_or(raw_window_handle::HandleError::Unavailable)?;
        let raw = if let Ok(display) = display.clone().downcast::<gdk4_wayland::WaylandDisplay>() {
            let display = display
                .wl_display_raw()
                .ok_or(raw_window_handle::HandleError::Unavailable)?;
            raw_window_handle::RawDisplayHandle::Wayland(
                raw_window_handle::WaylandDisplayHandle::new(display),
            )
        } else if let Ok(display) = display.downcast::<gdk4_x11::X11Display>() {
            // SAFETY: GDK owns this live Display for at least as long as the
            // returned borrowed handle. No ownership is transferred.
            let xdisplay = unsafe { display.xdisplay() };
            let xdisplay = NonNull::new(xdisplay.cast::<c_void>());
            raw_window_handle::RawDisplayHandle::Xlib(raw_window_handle::XlibDisplayHandle::new(
                xdisplay,
                display.screen().screen_number(),
            ))
        } else {
            return Err(raw_window_handle::HandleError::Unavailable);
        };
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw) })
    }
}

impl crate::PlatformWindow for Gtk4Window {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        self.0.borrow().window.is_maximized()
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.bounds().size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let width = finite_extent(size.width.0);
        let height = finite_extent(size.height.0);
        let (window, scene_picture) = {
            let mut state = self.0.borrow_mut();
            state.bounds.size = size;
            (state.window.clone(), state.scene_picture.clone())
        };
        window.set_default_size(width, height);
        scene_picture.set_size_request(width, height);
    }

    fn scale_factor(&self) -> f32 {
        self.0.borrow().scale_factor
    }

    fn appearance(&self) -> WindowAppearance {
        self.0.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        display_for_window(&self.0.borrow().window)
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.borrow().mouse_position
    }

    fn modifiers(&self) -> crate::Modifiers {
        self.0.borrow().modifiers
    }

    fn capslock(&self) -> Capslock {
        self.0.borrow().capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        let labels = answers
            .iter()
            .map(|answer| answer.label().to_string())
            .collect::<Vec<_>>();
        let cancel = answers
            .iter()
            .position(PromptButton::is_cancel)
            .map_or(-1, |index| index as i32);
        let mut builder = gtk4::AlertDialog::builder()
            .modal(true)
            .message(msg)
            .buttons(labels)
            .cancel_button(cancel);
        if let Some(detail) = detail {
            builder = builder.detail(detail);
        }
        let dialog = builder.build();
        let parent = self.0.borrow().window.clone();
        let (sender, receiver) = oneshot::channel();
        dialog.choose(Some(&parent), None::<&gio::Cancellable>, move |result| {
            let _ = sender.send(result.map_or(0, |index| index.max(0) as usize));
        });
        Some(receiver)
    }

    fn activate(&self) {
        let window = self.0.borrow().window.clone();
        window.present();
    }

    fn is_active(&self) -> bool {
        self.0.borrow().window.is_active()
    }

    fn is_hovered(&self) -> bool {
        self.0.borrow().hovered
    }

    fn set_title(&mut self, title: &str) {
        let window = self.0.borrow().window.clone();
        window.set_title(Some(title));
    }

    fn set_background_appearance(&self, background: WindowBackgroundAppearance) {
        self.0.borrow_mut().background = background;
    }

    fn set_opacity(&self, opacity: f32) {
        let window = self.0.borrow().window.clone();
        window.set_opacity(opacity.clamp(0.0, 1.0) as f64);
    }

    fn close(&self) {
        let window = self.0.borrow().window.clone();
        window.close();
    }

    fn minimize(&self) {
        let window = self.0.borrow().window.clone();
        window.minimize();
    }

    fn zoom(&self) {
        let window = self.0.borrow().window.clone();
        if window.is_maximized() {
            window.unmaximize();
        } else {
            window.maximize();
        }
    }

    fn toggle_fullscreen(&self) {
        let (window, fullscreen) = {
            let mut state = self.0.borrow_mut();
            state.fullscreen = !state.fullscreen;
            (state.window.clone(), state.fullscreen)
        };
        if fullscreen {
            window.fullscreen();
        } else {
            window.unfullscreen();
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.0.borrow().window.is_fullscreen()
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.borrow_mut().callbacks.request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.borrow_mut().callbacks.input = Some(callback);
    }

    fn game_input_capabilities(&self) -> GameInputCapabilities {
        let pointer_lock = if ensure_gtk_pointer_lock_backend(&self.0).is_ok() {
            GameInputAvailability::Available
        } else {
            GameInputAvailability::Unsupported
        };
        let gamepads = if cfg!(feature = "game-input") {
            GameInputAvailability::Available
        } else {
            GameInputAvailability::DisabledAtCompileTime
        };
        GameInputCapabilities::new(pointer_lock, gamepads)
    }

    fn pointer_lock_status(&self) -> PointerLockStatus {
        self.0.borrow().pointer_lock.status()
    }

    fn request_pointer_lock(&self) -> Result<(), GameInputError> {
        request_gtk_pointer_lock(&self.0)
    }

    fn exit_pointer_lock(&self) -> Result<(), GameInputError> {
        release_gtk_pointer_lock(&self.0)
    }

    fn pointer_lock_error(&self) -> Option<GameInputError> {
        self.0.borrow().pointer_lock.error()
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.borrow_mut().callbacks.active = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.borrow_mut().callbacks.hovered = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.borrow_mut().callbacks.resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().callbacks.moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.borrow_mut().callbacks.should_close = Some(callback);
    }

    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.0.borrow_mut().callbacks.hit_test_window_control = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.borrow_mut().callbacks.close = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().callbacks.appearance = Some(callback);
    }

    fn set_frame_polling(&self, active: bool) {
        set_window_frame_polling(&self.0, active);
    }

    fn sync_webviews(&mut self, webviews: &[PlatformWebView]) {
        let mut state = self.0.borrow_mut();
        let fixed = state.fixed.clone();
        sync_webviews(&fixed, &mut state.webviews, webviews);
    }

    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        dispatch_webview_command(&mut self.0.borrow_mut().webviews, command)
    }

    fn print(&mut self, job: crate::PlatformPrintJob) -> anyhow::Result<()> {
        super::print::print_silent(job)
    }

    fn show_print_dialog(&mut self, job: crate::PlatformPrintJob) -> anyhow::Result<()> {
        enum PrintParent {
            Wayland { surface: usize, display: usize },
            X11(std::os::raw::c_ulong),
        }

        let parent = match HasWindowHandle::window_handle(self)?.as_raw() {
            raw_window_handle::RawWindowHandle::Wayland(handle) => {
                let display = match HasDisplayHandle::display_handle(self)?.as_raw() {
                    raw_window_handle::RawDisplayHandle::Wayland(handle) => {
                        handle.display.as_ptr().cast::<c_void>() as usize
                    }
                    _ => anyhow::bail!(
                        "GTK4 Wayland print parent did not expose a Wayland display handle"
                    ),
                };
                PrintParent::Wayland {
                    surface: handle.surface.as_ptr().cast::<c_void>() as usize,
                    display,
                }
            }
            raw_window_handle::RawWindowHandle::Xlib(handle) => PrintParent::X11(handle.window),
            _ => {
                anyhow::bail!("GTK4 print parent did not expose a Wayland or X11 window handle")
            }
        };
        smol::block_on(async move {
            let parent = match parent {
                PrintParent::Wayland { surface, display } => {
                    // SAFETY: both pointers belong to this live `Gtk4Window`;
                    // the blocking portal operation completes before `self`
                    // can be released or either GDK-owned handle invalidated.
                    unsafe {
                        ashpd::WindowIdentifier::from_wayland_raw(
                            surface as *mut c_void,
                            display as *mut c_void,
                        )
                        .await
                    }
                }
                PrintParent::X11(xid) => Some(ashpd::WindowIdentifier::from_xid(xid)),
            };
            super::print::show_print_dialog(job, parent).await
        })
    }

    fn export_scene_png(
        &self,
        scene: &Scene,
    ) -> std::result::Result<Image, crate::WindowCaptureError> {
        if scene.has_live_surfaces() {
            return Err(crate::WindowCaptureError::LiveSurface);
        }
        let mut state = self.0.borrow_mut();
        let surface = state.window.surface().ok_or_else(|| {
            crate::WindowCaptureError::Backend(
                "GTK4 window surface is unavailable for scene capture".to_string(),
            )
        })?;
        let viewport = size(
            ScaledPixels(state.bounds.size.width.0),
            ScaledPixels(state.bounds.size.height.0),
        );
        let scale_factor = state.scale_factor;
        state
            .renderer
            .render_png(scene, viewport, scale_factor, &surface)
            .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))
    }

    fn draw(&self, scene: &Scene) {
        let mut state = self.0.borrow_mut();
        let viewport = size(
            ScaledPixels(state.bounds.size.width.0),
            ScaledPixels(state.bounds.size.height.0),
        );
        let scale_factor = state.scale_factor;
        match state
            .renderer
            .paintable_at_scale(scene, viewport, scale_factor)
        {
            Ok(paintable) => state.scene_picture.set_paintable(Some(&paintable)),
            Err(error) => log::error!("rendering Kael scene through GTK4/GSK failed: {error:#}"),
        }
    }

    fn sprite_atlas(&self) -> std::sync::Arc<dyn PlatformAtlas> {
        self.0.borrow().renderer.atlas()
    }

    fn request_decorations(&self, decorations: WindowDecorations) {
        let window = {
            let mut state = self.0.borrow_mut();
            state.decorations = decorations;
            state.window.clone()
        };
        window.set_decorated(decorations == WindowDecorations::Server);
    }

    fn window_decorations(&self) -> crate::Decorations {
        match self.0.borrow().decorations {
            WindowDecorations::Server => crate::Decorations::Server,
            WindowDecorations::Client => crate::Decorations::Client {
                tiling: crate::Tiling::default(),
            },
        }
    }

    fn show_window_menu(&self, _position: Point<Pixels>) {
        let state = self.0.borrow();
        let Some(event) = state.last_pointer_event.as_ref() else {
            return;
        };
        let Some(toplevel) = gtk_toplevel(&state.window) else {
            return;
        };
        let _ = toplevel.show_window_menu(event);
    }

    fn start_window_move(&self) {
        let state = self.0.borrow();
        let Some(event) = state.last_pointer_event.as_ref() else {
            return;
        };
        let Some(device) = event.device() else { return };
        let Some(toplevel) = gtk_toplevel(&state.window) else {
            return;
        };
        toplevel.begin_move(
            &device,
            1,
            f64::from(state.mouse_position.x.0),
            f64::from(state.mouse_position.y.0),
            event.time(),
        );
    }

    fn start_window_resize(&self, edge: ResizeEdge) {
        let state = self.0.borrow();
        let event = state.last_pointer_event.as_ref();
        let Some(toplevel) = gtk_toplevel(&state.window) else {
            return;
        };
        let device = event.and_then(gdk::Event::device);
        toplevel.begin_resize(
            surface_edge(edge),
            device.as_ref(),
            1,
            f64::from(state.mouse_position.x.0),
            f64::from(state.mouse_position.y.0),
            event.map_or(0, gdk::Event::time),
        );
    }

    fn map_window(&mut self) -> anyhow::Result<()> {
        let window = self.0.borrow().window.clone();
        window.present();
        Ok(())
    }

    fn set_app_id(&mut self, app_id: &str) {
        self.0.borrow_mut().app_id = Some(app_id.to_owned());
        apply_app_id(&self.0);
    }

    fn show(&self) {
        let window = self.0.borrow().window.clone();
        window.present();
    }

    fn hide(&self) {
        let window = self.0.borrow().window.clone();
        window.set_visible(false);
    }

    fn is_visible(&self) -> bool {
        self.0.borrow().window.is_visible()
    }

    fn set_mouse_passthrough(&self, passthrough: bool) {
        self.0.borrow_mut().mouse_passthrough = passthrough;
        apply_mouse_passthrough(&self.0);
    }

    fn set_atlas_byte_budget(&self, budget: Option<u64>) {
        self.0.borrow().renderer.set_atlas_byte_budget(budget);
    }

    fn display_refresh_rate(&self) -> Option<f32> {
        self.display().and_then(|display| display.refresh_rate())
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        let state = self.0.borrow();
        let renderer = state.window.renderer()?;
        let device_name = renderer.type_().name().to_string();
        let display_name = WidgetExt::display(&state.window).name().to_string();
        let renderer_override = std::env::var("GSK_RENDERER").ok();
        let driver_name = format!("GTK4/GSK on {display_name}");
        let driver_info = format!(
            "GTK {}.{}.{}; renderer override {}",
            gtk4::major_version(),
            gtk4::minor_version(),
            gtk4::micro_version(),
            renderer_override.as_deref().unwrap_or("automatic")
        );
        Some(GpuSpecs {
            is_software_emulated: gtk_renderer_is_software(
                &device_name,
                renderer_override.as_deref(),
            ),
            device_name,
            driver_name,
            driver_info,
        })
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        self.0
            .borrow()
            .ime_context
            .set_cursor_location(&gdk::Rectangle::new(
                bounds.origin.x.0.round() as i32,
                bounds.origin.y.0.round() as i32,
                bounds.size.width.0.max(1.0).round() as i32,
                bounds.size.height.0.max(1.0).round() as i32,
            ));
    }

    fn update_accessibility_tree(
        &mut self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        let state = self.0.borrow();
        state.accessibility_root.update_tree(tree);
        state.accessibility_root.drain_actions(tree)
    }
}

fn event_is_pointer_emulated(controller: &impl IsA<gtk4::EventController>) -> bool {
    controller
        .current_event()
        .is_some_and(|event| event.is_pointer_emulated())
}

fn set_window_hovered(state: &Rc<RefCell<Gtk4WindowState>>, hovered: bool) {
    let callback = {
        let mut state = state.borrow_mut();
        if state.hovered == hovered {
            return;
        }
        state.hovered = hovered;
        state.callbacks.hovered.take()
    };
    if let Some(mut callback) = callback {
        super::catch_platform_callback("window hover", (), || callback(hovered));
        state.borrow_mut().callbacks.hovered = Some(callback);
    }
}

fn webview_focus_owned(state: &Rc<RefCell<Gtk4WindowState>>) -> bool {
    let state = state.borrow();
    let focus = gtk4::prelude::RootExt::focus(&state.window);
    webview_owns_focus(&state.webviews, focus.as_ref())
}

fn dispatch_mouse_pointer(
    state: &Rc<RefCell<Gtk4WindowState>>,
    controller: &impl IsA<gtk4::EventController>,
    phase: PointerPhase,
    x: f64,
    y: f64,
    button: Option<MouseButton>,
    click_count: usize,
) {
    let position = point(
        px(finite_input(x).max(0.0) as f32),
        px(finite_input(y).max(0.0) as f32),
    );
    let (movement, buttons, modifiers, over_webview) = {
        let mut state = state.borrow_mut();
        let movement = position - state.mouse_position;
        state.mouse_position = position;
        let modifiers = modifiers_from_gdk(controller.current_event_state());
        state.modifiers = modifiers;
        state.last_pointer_event = controller.current_event();
        if let Some(button) = button {
            let mask = pointer_button(button);
            match phase {
                PointerPhase::Down => state.pointer_buttons.insert(mask),
                PointerPhase::Up | PointerPhase::Cancel => state.pointer_buttons.remove(mask),
                _ => {}
            }
        }
        (
            movement,
            state.pointer_buttons,
            modifiers,
            webview_intercepts_pointer(&state.webviews, position),
        )
    };
    if over_webview {
        return;
    }
    let pressure = if buttons.is_empty() { 0.0 } else { 0.5 };
    let _ = dispatch_window_input(
        state,
        PlatformInput::Pointer(PointerInputEvent {
            phase,
            pointer_id: PointerId::LEGACY_MOUSE,
            pointer_type: PointerType::Mouse,
            position,
            movement,
            button,
            buttons,
            modifiers,
            click_count,
            is_primary: true,
            pressure,
            tangential_pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 0.0,
            width: px(1.0),
            height: px(1.0),
            timestamp_ms: f64::from(controller.current_event_time()),
            coalesced: Vec::new(),
        }),
    );
}

fn dispatch_touch_event(
    state: &Rc<RefCell<Gtk4WindowState>>,
    event: &gdk::Event,
) -> glib::Propagation {
    let phase = match event.event_type() {
        gdk::EventType::TouchBegin => PointerPhase::Down,
        gdk::EventType::TouchUpdate => PointerPhase::Move,
        gdk::EventType::TouchEnd => PointerPhase::Up,
        gdk::EventType::TouchCancel => PointerPhase::Cancel,
        _ => return glib::Propagation::Proceed,
    };
    let Some((x, y)) = event.position() else {
        return glib::Propagation::Proceed;
    };
    let position = point(
        px(finite_input(x).max(0.0) as f32),
        px(finite_input(y).max(0.0) as f32),
    );
    let sequence = event.event_sequence();
    let (touch, movement, modifiers, over_webview) = {
        let mut state = state.borrow_mut();
        let over_webview = webview_intercepts_pointer(&state.webviews, position);
        let modifiers = modifiers_from_gdk(event.modifier_state());
        state.modifiers = modifiers;
        state.last_pointer_event = Some(event.clone());
        match phase {
            PointerPhase::Down if over_webview => {
                return glib::Propagation::Proceed;
            }
            PointerPhase::Down => {
                let touch = Gtk4TouchState {
                    id: PointerId::new(state.next_touch_id),
                    position,
                    is_primary: state.touches.is_empty(),
                };
                state.next_touch_id = state.next_touch_id.saturating_add(1);
                state.touches.insert(sequence, touch);
                (touch, Point::default(), modifiers, false)
            }
            PointerPhase::Move => {
                let Some(touch) = state.touches.get_mut(&sequence) else {
                    return glib::Propagation::Proceed;
                };
                let movement = position - touch.position;
                touch.position = position;
                (*touch, movement, modifiers, over_webview)
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                let Some(touch) = state.touches.remove(&sequence) else {
                    return glib::Propagation::Proceed;
                };
                let movement = position - touch.position;
                (touch, movement, modifiers, over_webview)
            }
            _ => return glib::Propagation::Proceed,
        }
    };
    if over_webview {
        return glib::Propagation::Proceed;
    }
    if phase == PointerPhase::Down {
        state.borrow().fixed.grab_focus();
    }
    let active = matches!(phase, PointerPhase::Down | PointerPhase::Move);
    dispatch_window_input(
        state,
        PlatformInput::Pointer(PointerInputEvent {
            phase,
            pointer_id: touch.id,
            pointer_type: PointerType::Touch,
            position,
            movement,
            button: matches!(phase, PointerPhase::Down | PointerPhase::Up)
                .then_some(MouseButton::Left),
            buttons: if active {
                PointerButtons::PRIMARY
            } else {
                PointerButtons::empty()
            },
            modifiers,
            click_count: usize::from(matches!(phase, PointerPhase::Down | PointerPhase::Up)),
            is_primary: touch.is_primary,
            pressure: if active { 0.5 } else { 0.0 },
            tangential_pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 0.0,
            width: px(1.0),
            height: px(1.0),
            timestamp_ms: f64::from(event.time()),
            coalesced: Vec::new(),
        }),
    )
}

fn dispatch_window_input(
    state: &Rc<RefCell<Gtk4WindowState>>,
    input: PlatformInput,
) -> glib::Propagation {
    let callback = state.borrow_mut().callbacks.input.take();
    let result = if let Some(mut callback) = callback {
        let result = super::catch_platform_callback(
            "window input",
            DispatchEventResult {
                propagate: true,
                default_prevented: false,
            },
            || callback(input.clone()),
        );
        state.borrow_mut().callbacks.input = Some(callback);
        result
    } else {
        DispatchEventResult {
            propagate: true,
            default_prevented: false,
        }
    };

    if result.propagate
        && let PlatformInput::KeyDown(event) = input
        && event.keystroke.modifiers.is_subset_of(&Modifiers::shift())
        && let Some(key_char) = event.keystroke.key_char
    {
        with_input_handler(state, |handler| {
            handler.replace_text_in_range(None, &key_char)
        });
    }

    if result.propagate && !result.default_prevented {
        glib::Propagation::Proceed
    } else {
        glib::Propagation::Stop
    }
}

fn with_input_handler(
    state: &Rc<RefCell<Gtk4WindowState>>,
    callback: impl FnOnce(&mut PlatformInputHandler),
) {
    let handler = state.borrow_mut().input_handler.take();
    if let Some(mut handler) = handler {
        callback(&mut handler);
        state.borrow_mut().input_handler = Some(handler);
    }
}

fn modifiers_from_gdk(state: gdk::ModifierType) -> Modifiers {
    Modifiers {
        control: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        platform: state.intersects(
            gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::HYPER_MASK
                | gdk::ModifierType::META_MASK,
        ),
        function: false,
    }
}

fn pointer_button(button: MouseButton) -> PointerButtons {
    match button {
        MouseButton::Left => PointerButtons::PRIMARY,
        MouseButton::Right => PointerButtons::SECONDARY,
        MouseButton::Middle => PointerButtons::AUXILIARY,
        MouseButton::Navigate(NavigationDirection::Back) => PointerButtons::BACK,
        MouseButton::Navigate(NavigationDirection::Forward) => PointerButtons::FORWARD,
    }
}

fn mouse_button(button: u32) -> Option<MouseButton> {
    match button {
        1 => Some(MouseButton::Left),
        2 => Some(MouseButton::Middle),
        3 => Some(MouseButton::Right),
        8 => Some(MouseButton::Navigate(NavigationDirection::Back)),
        9 => Some(MouseButton::Navigate(NavigationDirection::Forward)),
        _ => None,
    }
}

fn keystroke_from_gdk(keyval: gdk::Key, mut modifiers: Modifiers) -> Keystroke {
    let name = keyval
        .name()
        .map_or_else(String::new, |name| name.to_string());
    let key_char = keyval
        .to_unicode()
        .filter(|character| !character.is_control())
        .filter(|_| !modifiers.control && !modifiers.platform)
        .map(|character| character.to_string());
    let key = match name.as_str() {
        "Return" | "KP_Enter" => "enter".to_string(),
        "Page_Up" | "KP_Page_Up" | "Prior" | "KP_Prior" => "pageup".to_string(),
        "Page_Down" | "KP_Page_Down" | "Next" | "KP_Next" => "pagedown".to_string(),
        "ISO_Left_Tab" | "Tab" => "tab".to_string(),
        "BackSpace" => "backspace".to_string(),
        "Delete" | "KP_Delete" => "delete".to_string(),
        "Escape" => "escape".to_string(),
        "space" | "KP_Space" => "space".to_string(),
        "Left" | "KP_Left" => "left".to_string(),
        "Right" | "KP_Right" => "right".to_string(),
        "Up" | "KP_Up" => "up".to_string(),
        "Down" | "KP_Down" => "down".to_string(),
        "Home" | "KP_Home" => "home".to_string(),
        "End" | "KP_End" => "end".to_string(),
        "Insert" | "KP_Insert" => "insert".to_string(),
        "XF86Back" => "back".to_string(),
        "XF86Forward" => "forward".to_string(),
        "XF86Cut" => "cut".to_string(),
        "XF86Copy" => "copy".to_string(),
        "XF86Paste" => "paste".to_string(),
        "XF86New" => "new".to_string(),
        "XF86Open" => "open".to_string(),
        "XF86Save" => "save".to_string(),
        _ => key_char
            .as_ref()
            .filter(|character| character.chars().count() == 1)
            .map(|character| character.to_lowercase())
            .unwrap_or_else(|| {
                name.strip_prefix("KP_")
                    .unwrap_or(&name)
                    .replace('_', "")
                    .to_lowercase()
            }),
    };
    if modifiers.shift && key.chars().count() == 1 && key.to_lowercase() == key.to_uppercase() {
        modifiers.shift = false;
    }
    Keystroke {
        modifiers,
        key,
        key_char,
    }
}

fn finite_input(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-1_000_000.0, 1_000_000.0)
    } else {
        0.0
    }
}

fn safe_position(x: f64, y: f64) -> Point<Pixels> {
    point(
        px(finite_input(x).max(0.0) as f32),
        px(finite_input(y).max(0.0) as f32),
    )
}

fn gtk_toplevel(window: &ApplicationWindow) -> Option<gdk::Toplevel> {
    window.surface()?.downcast::<gdk::Toplevel>().ok()
}

fn apply_app_id(state: &Rc<RefCell<Gtk4WindowState>>) {
    let (window, app_id) = {
        let state = state.borrow();
        (state.window.clone(), state.app_id.clone())
    };
    let Some(app_id) = app_id else {
        return;
    };
    let Some(surface) = window.surface() else {
        return;
    };
    let Ok(toplevel) = surface.downcast::<gdk4_wayland::WaylandToplevel>() else {
        return;
    };
    toplevel.set_application_id(&app_id);
}

fn surface_edge(edge: ResizeEdge) -> gdk::SurfaceEdge {
    match edge {
        ResizeEdge::Top => gdk::SurfaceEdge::North,
        ResizeEdge::TopRight => gdk::SurfaceEdge::NorthEast,
        ResizeEdge::Right => gdk::SurfaceEdge::East,
        ResizeEdge::BottomRight => gdk::SurfaceEdge::SouthEast,
        ResizeEdge::Bottom => gdk::SurfaceEdge::South,
        ResizeEdge::BottomLeft => gdk::SurfaceEdge::SouthWest,
        ResizeEdge::Left => gdk::SurfaceEdge::West,
        ResizeEdge::TopLeft => gdk::SurfaceEdge::NorthWest,
    }
}

fn apply_mouse_passthrough(state: &Rc<RefCell<Gtk4WindowState>>) {
    let (window, mouse_passthrough) = {
        let state = state.borrow();
        (state.window.clone(), state.mouse_passthrough)
    };
    let Some(surface) = window.surface() else {
        return;
    };
    if mouse_passthrough {
        let region = gtk4::cairo::Region::create();
        surface.set_input_region(Some(&region));
    } else {
        surface.set_input_region(None::<&gtk4::cairo::Region>);
    }
}

fn drop_target_paths(target: &gtk4::DropTarget) -> Option<ExternalPaths> {
    target
        .value()?
        .get::<gdk::FileList>()
        .ok()
        .map(paths_from_file_list)
}

fn paths_from_file_list(files: gdk::FileList) -> ExternalPaths {
    ExternalPaths::from_paths(files.files().into_iter().filter_map(|file| file.path()))
}

type Gtk4ContextMenuCallback = Rc<RefCell<Box<dyn FnMut(SharedString)>>>;

fn show_gtk4_context_menu(
    state: &Rc<RefCell<Gtk4WindowState>>,
    position: Point<Pixels>,
    items: Vec<TrayMenuItem>,
    callback: Box<dyn FnMut(SharedString)>,
) {
    dismiss_gtk4_context_menu(state);
    let fixed = state.borrow().fixed.clone();
    let popover = gtk4::Popover::new();
    popover.set_autohide(true);
    popover.set_has_arrow(false);
    popover.add_css_class("menu");
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.set_margin_top(4);
    content.set_margin_bottom(4);
    content.set_margin_start(4);
    content.set_margin_end(4);
    let callback = Rc::new(RefCell::new(callback));
    let selected = Rc::new(Cell::new(false));
    populate_gtk4_context_menu(&content, &items, &popover, &callback, &selected);
    popover.set_child(Some(&content));
    let x = position
        .x
        .0
        .round()
        .clamp(0.0, fixed.width().max(1) as f32 - 1.0) as i32;
    let y = position
        .y
        .0
        .round()
        .clamp(0.0, fixed.height().max(1) as f32 - 1.0) as i32;
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x, y, 1, 1)));
    popover.set_parent(&fixed);
    state.borrow_mut().context_menu = Some(popover.clone());
    let weak = Rc::downgrade(state);
    popover.connect_closed(move |popover| {
        if let Some(state) = weak.upgrade() {
            let is_current = state
                .borrow()
                .context_menu
                .as_ref()
                .is_some_and(|current| current == popover);
            if is_current {
                state.borrow_mut().context_menu.take();
            }
        }
        popover.unparent();
    });
    popover.popup();
}

fn dismiss_gtk4_context_menu(state: &Rc<RefCell<Gtk4WindowState>>) {
    let popover = { state.borrow_mut().context_menu.take() };
    if let Some(popover) = popover {
        popover.popdown();
    }
}

fn populate_gtk4_context_menu(
    content: &gtk4::Box,
    items: &[TrayMenuItem],
    root: &gtk4::Popover,
    callback: &Gtk4ContextMenuCallback,
    selected: &Rc<Cell<bool>>,
) {
    for item in items {
        match item {
            TrayMenuItem::Action { label, id } => {
                let button = gtk4::Button::with_label(label.as_ref());
                button.add_css_class("flat");
                button.set_hexpand(true);
                button.set_halign(gtk4::Align::Fill);
                let callback = callback.clone();
                let selected = selected.clone();
                let root = root.clone();
                let id = id.clone();
                button.connect_clicked(move |_| {
                    if !selected.replace(true) {
                        super::catch_platform_callback("GTK4 context menu", (), || {
                            callback.borrow_mut()(id.clone())
                        });
                    }
                    root.popdown();
                });
                content.append(&button);
            }
            TrayMenuItem::Toggle { label, checked, id } => {
                let toggle = gtk4::CheckButton::with_label(label.as_ref());
                toggle.set_active(*checked);
                toggle.set_hexpand(true);
                toggle.set_halign(gtk4::Align::Fill);
                let callback = callback.clone();
                let selected = selected.clone();
                let root = root.clone();
                let id = id.clone();
                toggle.connect_toggled(move |_| {
                    if !selected.replace(true) {
                        super::catch_platform_callback("GTK4 context menu toggle", (), || {
                            callback.borrow_mut()(id.clone())
                        });
                    }
                    root.popdown();
                });
                content.append(&toggle);
            }
            TrayMenuItem::Separator => {
                content.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
            }
            TrayMenuItem::Submenu { label, items } => {
                let menu_button = gtk4::MenuButton::new();
                menu_button.set_label(label.as_ref());
                menu_button.add_css_class("flat");
                menu_button.set_hexpand(true);
                menu_button.set_halign(gtk4::Align::Fill);
                let submenu = gtk4::Popover::new();
                submenu.set_autohide(true);
                submenu.add_css_class("menu");
                let submenu_content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
                submenu_content.set_margin_top(4);
                submenu_content.set_margin_bottom(4);
                submenu_content.set_margin_start(4);
                submenu_content.set_margin_end(4);
                populate_gtk4_context_menu(&submenu_content, items, root, callback, selected);
                submenu.set_child(Some(&submenu_content));
                menu_button.set_popover(Some(&submenu));
                content.append(&menu_button);
            }
        }
    }
}

fn connect_surface_metric_signals(state: &Rc<RefCell<Gtk4WindowState>>) {
    let Some(surface) = state.borrow().window.surface() else {
        return;
    };

    let refresh = |weak: &Weak<RefCell<Gtk4WindowState>>| {
        let Some(state) = weak.upgrade() else { return };
        let fixed = state.borrow().fixed.clone();
        update_window_metrics(&state, &fixed);
    };

    let weak = Rc::downgrade(state);
    surface.connect_width_notify(move |_| refresh(&weak));
    let weak = Rc::downgrade(state);
    surface.connect_height_notify(move |_| refresh(&weak));
    let weak = Rc::downgrade(state);
    surface.connect_scale_factor_notify(move |_| refresh(&weak));
    let weak = Rc::downgrade(state);
    surface.connect_scale_notify(move |_| refresh(&weak));
    let weak = Rc::downgrade(state);
    surface.connect_enter_monitor(move |_, _| refresh(&weak));
    let weak = Rc::downgrade(state);
    surface.connect_leave_monitor(move |_, _| refresh(&weak));
}

fn set_window_frame_polling(state: &Rc<RefCell<Gtk4WindowState>>, active: bool) {
    state.borrow_mut().frame_polling = active;
    sync_window_frame_tick(state);
}

fn window_needs_frame_tick(state: &Gtk4WindowState) -> bool {
    frame_tick_required(state.frame_polling, state.pointer_lock.status())
}

fn frame_tick_required(frame_polling: bool, pointer_lock: PointerLockStatus) -> bool {
    frame_polling
        || matches!(
            pointer_lock,
            PointerLockStatus::Requesting | PointerLockStatus::Locked
        )
}

fn sync_window_frame_tick(state: &Rc<RefCell<Gtk4WindowState>>) {
    let active = window_needs_frame_tick(&state.borrow());
    let existing = if active {
        None
    } else {
        state.borrow_mut().frame_tick_id.take()
    };
    if let Some(existing) = existing {
        existing.remove();
        return;
    }
    if !active || state.borrow().frame_tick_id.is_some() {
        return;
    }

    let fixed = state.borrow().fixed.clone();
    let weak = Rc::downgrade(state);
    let tick_id = fixed.add_tick_callback(move |_, _| {
        let Some(state) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !state.borrow().frame_polling {
            let backend = state.borrow().pointer_lock_backend.clone();
            if let Some(backend) = backend {
                let motions = backend.borrow_mut().dispatch_pending();
                for motion in motions {
                    dispatch_gtk_relative_motion(motion);
                }
            }
            if !window_needs_frame_tick(&state.borrow()) {
                state.borrow_mut().frame_tick_id.take();
                return glib::ControlFlow::Break;
            }
        } else if let Some(backend) = state.borrow().pointer_lock_backend.clone() {
            let motions = backend.borrow_mut().dispatch_pending();
            for motion in motions {
                dispatch_gtk_relative_motion(motion);
            }
        }
        GTK4_FRAME_TICKS.fetch_add(1, Ordering::Relaxed);
        request_window_frame(&state);
        glib::ControlFlow::Continue
    });
    state.borrow_mut().frame_tick_id = Some(tick_id);
}

fn dispatch_gtk_relative_motion(motion: Gtk4RelativeMotion) {
    let Some(window) = motion.target.window() else {
        return;
    };
    let (position, pressed_button, buttons, modifiers, scale_factor) = {
        let state = window.borrow();
        if state.pointer_lock_generation != motion.target.generation
            || state.pointer_lock.status() != PointerLockStatus::Locked
        {
            return;
        }
        (
            state.mouse_position,
            state.pointer_buttons.primary_legacy_button(),
            state.pointer_buttons,
            state.modifiers,
            state.scale_factor,
        )
    };
    let mouse_move = MouseMoveEvent {
        position,
        pressed_button,
        modifiers,
    };
    let mut pointer = PointerInputEvent::from(&mouse_move);
    pointer.buttons = buttons;
    let scale = if motion.device_pixels {
        f64::from(valid_scale(scale_factor))
    } else {
        1.0
    };
    pointer.movement = point(
        px(((motion.dx / scale) as f32).clamp(-1_000_000.0, 1_000_000.0)),
        px(((motion.dy / scale) as f32).clamp(-1_000_000.0, 1_000_000.0)),
    );
    pointer.timestamp_ms = motion.timestamp_ms;
    let _ = dispatch_window_input(&window, PlatformInput::Pointer(pointer));
}

fn xinput_raw_axis(event: &xinput::RawMotionEvent, valuator_number: u16) -> f64 {
    xinput_raw_axis_values(&event.valuator_mask, &event.axisvalues_raw, valuator_number)
}

fn xinput_raw_axis_values(
    valuator_mask: &[u32],
    axisvalues_raw: &[xinput::Fp3232],
    valuator_number: u16,
) -> f64 {
    let word = usize::from(valuator_number / 32);
    let bit = u32::from(valuator_number % 32);
    let Some(mask_word) = valuator_mask.get(word) else {
        return 0.0;
    };
    if mask_word & (1_u32 << bit) == 0 {
        return 0.0;
    }
    let preceding_words = valuator_mask
        .iter()
        .take(word)
        .map(|word| word.count_ones() as usize)
        .sum::<usize>();
    let preceding_bits = (mask_word & ((1_u32 << bit).wrapping_sub(1))).count_ones() as usize;
    axisvalues_raw
        .get(preceding_words + preceding_bits)
        .map(|value| f64::from(value.integral) + f64::from(value.frac) / (u32::MAX as f64 + 1.0))
        .unwrap_or(0.0)
}

fn ensure_gtk_pointer_lock_backend(
    state: &Rc<RefCell<Gtk4WindowState>>,
) -> Result<Rc<RefCell<Gtk4PointerLockBackend>>, GameInputError> {
    if let Some(backend) = state.borrow().pointer_lock_backend.clone() {
        return Ok(backend);
    }
    let backend = Gtk4PointerLockBackend::new().map_err(|error| {
        GameInputError::new(
            GameInputErrorKind::Unsupported,
            format!("GTK4 native pointer lock is unavailable: {error:#}"),
        )
    })?;
    let backend = Rc::new(RefCell::new(backend));
    state.borrow_mut().pointer_lock_backend = Some(backend.clone());
    Ok(backend)
}

fn request_gtk_pointer_lock(state: &Rc<RefCell<Gtk4WindowState>>) -> Result<(), GameInputError> {
    // Native menus own the pointer grab while open. Dismiss them before
    // requesting confinement so pointer lock can replace that grab on X11 and
    // starts from an unambiguous pointer focus on Wayland.
    dismiss_gtk4_context_menu(state);
    {
        let mut state = state.borrow_mut();
        if !state.pointer_lock.begin_request()? {
            return Ok(());
        }
        let wayland_needs_focused_pointer = gdk::Display::default()
            .is_some_and(|display| display.is::<gdk4_wayland::WaylandDisplay>());
        if !state.window.is_active() || (wayland_needs_focused_pointer && !state.hovered) {
            let error = GameInputError::new(
                GameInputErrorKind::Rejected,
                if wayland_needs_focused_pointer {
                    "GTK4 Wayland pointer lock requires an active, pointer-focused window"
                } else {
                    "GTK4 X11 pointer lock requires an active window"
                },
            );
            return Err(state.pointer_lock.fail(error));
        }
    }

    let backend = match ensure_gtk_pointer_lock_backend(state) {
        Ok(backend) => backend,
        Err(error) => return Err(state.borrow_mut().pointer_lock.fail(error)),
    };

    let target = {
        let mut window_state = state.borrow_mut();
        window_state.pointer_lock_generation =
            window_state.pointer_lock_generation.wrapping_add(1).max(1);
        Gtk4PointerLockTarget {
            token: window_state.pointer_lock_token,
            generation: window_state.pointer_lock_generation,
        }
    };
    let result = (|| {
        let mut backend = backend.borrow_mut();
        match &mut *backend {
            Gtk4PointerLockBackend::Wayland(backend) => {
                let (surface, pointer) = {
                    let state = state.borrow();
                    let surface = state
                        .window
                        .surface()
                        .context("GTK4 window surface is unavailable")?
                        .downcast::<gdk4_wayland::WaylandSurface>()
                        .map_err(|_| anyhow::anyhow!("GTK4 window is not using a Wayland surface"))?
                        .wl_surface()
                        .context("GTK4 did not expose its wl_surface")?;
                    let pointer = state
                        .last_pointer_event
                        .as_ref()
                        .and_then(gdk::Event::device)
                        .context("GTK4 has no focused pointer device for this window")?
                        .downcast::<gdk4_wayland::WaylandDevice>()
                        .map_err(|_| {
                            anyhow::anyhow!("GTK4 pointer device is not a Wayland device")
                        })?
                        .wl_pointer()
                        .context("GTK4 did not expose its wl_pointer")?;
                    (surface, pointer)
                };
                backend.clear_request();
                let queue_handle = backend.event_queue.handle();
                backend.relative_pointer = Some(backend.relative_manager.get_relative_pointer(
                    &pointer,
                    &queue_handle,
                    target.clone(),
                ));
                backend.locked_pointer = Some(backend.constraints.lock_pointer(
                    &surface,
                    &pointer,
                    None,
                    zwp_pointer_constraints_v1::Lifetime::Persistent,
                    &queue_handle,
                    target,
                ));
                backend
                    .connection
                    .flush()
                    .context("flushing Wayland pointer-lock request")?;
            }
            Gtk4PointerLockBackend::X11(backend) => {
                let window = state
                    .borrow()
                    .window
                    .surface()
                    .context("GTK4 window surface is unavailable")?
                    .downcast::<gdk4_x11::X11Surface>()
                    .map_err(|_| anyhow::anyhow!("GTK4 window is not using an X11 surface"))?
                    .xid() as u32;
                backend.request(window, target)?;
                let mut state = state.borrow_mut();
                state.pointer_lock.lock();
                state
                    .fixed
                    .set_cursor_from_name(cursor_name(CursorStyle::None));
            }
        }
        Ok::<(), anyhow::Error>(())
    })();

    if let Err(error) = result {
        let error = GameInputError::new(
            GameInputErrorKind::InitializationFailed,
            format!("GTK4 pointer-lock request failed: {error:#}"),
        );
        return Err(state.borrow_mut().pointer_lock.fail(error));
    }
    sync_window_frame_tick(state);
    Ok(())
}

fn release_gtk_pointer_lock(state: &Rc<RefCell<Gtk4WindowState>>) -> Result<(), GameInputError> {
    let cleanup_error = state
        .borrow()
        .pointer_lock_backend
        .clone()
        .and_then(|backend| backend.borrow_mut().clear_request().err());
    let (fixed, cursor) = {
        let mut state = state.borrow_mut();
        state.pointer_lock.unlock();
        (state.fixed.clone(), cursor_name(state.cursor_style))
    };
    fixed.set_cursor_from_name(cursor);
    sync_window_frame_tick(state);
    if let Some(error) = cleanup_error {
        let error = GameInputError::new(
            GameInputErrorKind::Platform,
            format!("GTK4 pointer-lock cleanup failed: {error:#}"),
        );
        Err(state.borrow_mut().pointer_lock.fail(error))
    } else {
        Ok(())
    }
}

fn update_window_metrics(state: &Rc<RefCell<Gtk4WindowState>>, fixed: &Fixed) {
    let width = fixed.width().max(0);
    let height = fixed.height().max(0);
    let (display, scale_factor, monitor_id) = {
        let state = state.borrow();
        let display = display_for_window(&state.window);
        let scale = display
            .as_ref()
            .map_or(state.scale_factor, |display| display.scale_factor());
        let monitor_id = display.as_ref().map(|display| display.id());
        (display, valid_scale(scale), monitor_id)
    };
    let _ = display;

    let (scene_picture, schedule_dispatch) = {
        let mut state = state.borrow_mut();
        let size = Size {
            width: px(width as f32),
            height: px(height as f32),
        };
        let size_changed = state.bounds.size != size;
        let scale_changed = (state.scale_factor - scale_factor).abs() > f32::EPSILON;
        let monitor_changed = state.monitor_id != monitor_id;
        if !(size_changed || scale_changed || monitor_changed) {
            return;
        }
        state.bounds.size = size;
        state.scale_factor = scale_factor;
        state.monitor_id = monitor_id;
        state.pending_resize |= size_changed || scale_changed;
        state.pending_moved |= monitor_changed;
        state.pending_metric_frame |= scale_changed || monitor_changed;
        let schedule_dispatch = !state.metric_dispatch_scheduled;
        state.metric_dispatch_scheduled = true;
        (state.scene_picture.clone(), schedule_dispatch)
    };
    scene_picture.set_size_request(width, height);
    if schedule_dispatch {
        let weak = Rc::downgrade(state);
        glib::idle_add_local_once(move || {
            let Some(state) = weak.upgrade() else { return };
            dispatch_deferred_window_metrics(&state);
        });
    }
}

fn dispatch_deferred_window_metrics(state: &Rc<RefCell<Gtk4WindowState>>) {
    let (resize_callback, moved_callback, size, scale_factor, request_frame) = {
        let mut state = state.borrow_mut();
        state.metric_dispatch_scheduled = false;
        let resize = std::mem::take(&mut state.pending_resize);
        let moved = std::mem::take(&mut state.pending_moved);
        let request_frame = std::mem::take(&mut state.pending_metric_frame);
        (
            resize.then(|| state.callbacks.resize.take()).flatten(),
            moved.then(|| state.callbacks.moved.take()).flatten(),
            state.bounds.size,
            state.scale_factor,
            request_frame,
        )
    };

    if let Some(mut callback) = resize_callback {
        super::catch_platform_callback("window resize or backing scale", (), || {
            callback(size, scale_factor)
        });
        state.borrow_mut().callbacks.resize = Some(callback);
    }
    if let Some(mut callback) = moved_callback {
        super::catch_platform_callback("window moved between displays", (), &mut callback);
        state.borrow_mut().callbacks.moved = Some(callback);
    }
    if request_frame {
        request_window_frame_force(state);
    }
}

fn defer_window_active(state: &Rc<RefCell<Gtk4WindowState>>, active: bool) {
    let schedule = {
        let mut state = state.borrow_mut();
        state.pending_active = Some(active);
        let schedule = !state.active_dispatch_scheduled;
        state.active_dispatch_scheduled = true;
        schedule
    };
    if !schedule {
        return;
    }

    let weak = Rc::downgrade(state);
    glib::idle_add_local_once(move || {
        let Some(state) = weak.upgrade() else { return };
        let (active, callback) = {
            let mut state = state.borrow_mut();
            state.active_dispatch_scheduled = false;
            (state.pending_active.take(), state.callbacks.active.take())
        };
        if let (Some(active), Some(mut callback)) = (active, callback) {
            super::catch_platform_callback("window active", (), || callback(active));
            state.borrow_mut().callbacks.active = Some(callback);
        }
    });
}

fn request_window_frame(state: &Rc<RefCell<Gtk4WindowState>>) {
    if !state.borrow().frame_polling {
        return;
    }
    request_window_frame_with_options(state, RequestFrameOptions::default());
}

fn request_window_frame_force(state: &Rc<RefCell<Gtk4WindowState>>) {
    request_window_frame_with_options(
        state,
        RequestFrameOptions {
            require_presentation: false,
            force_render: true,
        },
    );
}

fn request_window_frame_with_options(
    state: &Rc<RefCell<Gtk4WindowState>>,
    options: RequestFrameOptions,
) {
    let callback = state.borrow_mut().callbacks.request_frame.take();
    if let Some(mut callback) = callback {
        super::catch_platform_callback("GTK frame clock", (), || callback(options));
        state.borrow_mut().callbacks.request_frame = Some(callback);
    }
}

fn write_clipboard(primary: bool, item: ClipboardItem) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let clipboard = if primary {
        display.primary_clipboard()
    } else {
        display.clipboard()
    };
    let mut providers = Vec::new();
    if let Some(text) = item.text().filter(|text| !text.is_empty()) {
        providers.push(gdk::ContentProvider::for_value(&text.to_value()));
    }
    if let Some(html) = item.html() {
        providers.push(content_provider_for_bytes("text/html", html.into_bytes()));
    }
    if !item.has_html()
        && let Some(metadata) = item.metadata()
    {
        providers.push(content_provider_for_bytes(
            GTK_CLIPBOARD_METADATA_MIME,
            metadata.as_bytes().to_vec(),
        ));
    }
    let mut image_formats = FxHashSet::default();
    for image in item.images() {
        if image.has_bytes() && image_formats.insert(image.format()) {
            providers.push(content_provider_for_bytes(
                image.format().mime_type(),
                image.bytes().to_vec(),
            ));
        }
    }
    if providers.is_empty() {
        log::warn!("refusing to publish an empty GTK4 clipboard payload");
        return;
    }
    let provider = if providers.len() == 1 {
        providers.pop().expect("provider count was checked")
    } else {
        gdk::ContentProvider::new_union(&providers)
    };
    if let Err(error) = clipboard.set_content(Some(&provider)) {
        log::warn!("publishing rich GTK4 clipboard content failed: {error}");
    }
}

fn read_clipboard(primary: bool) -> Option<ClipboardItem> {
    let display = gdk::Display::default()?;
    let clipboard = if primary {
        display.primary_clipboard()
    } else {
        display.clipboard()
    };
    let text = read_clipboard_text(&clipboard).filter(|text| !text.is_empty());
    let html = read_clipboard_mime(&clipboard, "text/html", GTK_CLIPBOARD_HTML_LIMIT)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|html| !html.trim().is_empty());
    let metadata = html
        .is_none()
        .then(|| {
            read_clipboard_mime(
                &clipboard,
                GTK_CLIPBOARD_METADATA_MIME,
                GTK_CLIPBOARD_HTML_LIMIT,
            )
        })
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|metadata| !metadata.is_empty());
    let image = preferred_clipboard_image(&clipboard);
    if text.is_none() && html.is_none() && image.is_none() {
        return None;
    }

    let mut builder = ClipboardItem::builder();
    match (text, html, metadata) {
        (Some(text), Some(html), _) => {
            builder = builder.html(text, html).ok()?;
        }
        (Some(text), None, Some(metadata)) => {
            builder = builder.text_with_metadata(text, metadata);
        }
        (Some(text), None, None) => {
            builder = builder.text(text);
        }
        (None, Some(html), _) => {
            // HTML-only owners are uncommon but valid. Preserve the rich
            // representation and use its source as a lossless text fallback.
            builder = builder.html(html.clone(), html).ok()?;
        }
        (None, None, _) => {}
    }
    if let Some(image) = image {
        builder = builder.image(image);
    }
    builder.build_checked().ok()
}

fn content_provider_for_bytes(mime_type: &str, bytes: Vec<u8>) -> gdk::ContentProvider {
    gdk::ContentProvider::for_bytes(mime_type, &glib::Bytes::from_owned(bytes))
}

fn read_clipboard_text(clipboard: &gdk::Clipboard) -> Option<String> {
    read_clipboard_mimes(
        clipboard,
        &["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"],
        GTK_CLIPBOARD_TEXT_LIMIT,
    )
    .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn read_clipboard_mime(
    clipboard: &gdk::Clipboard,
    mime_type: &str,
    byte_limit: usize,
) -> Option<Vec<u8>> {
    read_clipboard_mimes(clipboard, &[mime_type], byte_limit)
}

fn read_clipboard_mimes(
    clipboard: &gdk::Clipboard,
    mime_types: &[&str],
    byte_limit: usize,
) -> Option<Vec<u8>> {
    let formats = clipboard.formats();
    if !mime_types
        .iter()
        .any(|mime_type| formats.contain_mime_type(mime_type))
    {
        return None;
    }
    let result = Rc::new(RefCell::new(None));
    let result_out = result.clone();
    let main_loop = glib::MainLoop::new(None, false);
    let finished_loop = main_loop.clone();
    clipboard.read_async(
        mime_types,
        glib::Priority::DEFAULT,
        None::<&gio::Cancellable>,
        move |stream| {
            *result_out.borrow_mut() = Some(
                stream
                    .map_err(anyhow::Error::from)
                    .and_then(|(stream, _)| read_stream_bounded(&stream, byte_limit)),
            );
            finished_loop.quit();
        },
    );
    main_loop.run();
    match result.borrow_mut().take()? {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            log::warn!("reading GTK4 clipboard content failed: {error:#}");
            None
        }
    }
}

fn read_stream_bounded(stream: &gio::InputStream, byte_limit: usize) -> anyhow::Result<Vec<u8>> {
    const CHUNK_SIZE: usize = 64 * 1024;
    let mut output = Vec::new();
    loop {
        let remaining = byte_limit.saturating_sub(output.len());
        let chunk = stream.read_bytes(
            remaining.saturating_add(1).min(CHUNK_SIZE),
            None::<&gio::Cancellable>,
        )?;
        if chunk.is_empty() {
            break;
        }
        output.extend_from_slice(&chunk);
        anyhow::ensure!(
            output.len() <= byte_limit,
            "clipboard representation exceeds its {byte_limit}-byte limit"
        );
    }
    Ok(output)
}

fn preferred_clipboard_image(clipboard: &gdk::Clipboard) -> Option<Image> {
    for format in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Webp,
        ImageFormat::Gif,
        ImageFormat::Svg,
        ImageFormat::Bmp,
        ImageFormat::Tiff,
    ] {
        let Some(bytes) =
            read_clipboard_mime(clipboard, format.mime_type(), GTK_CLIPBOARD_IMAGE_LIMIT)
        else {
            continue;
        };
        let image = Image::from_bytes(format, bytes);
        if let Err(error) = image.validate() {
            log::warn!(
                "ignoring invalid GTK4 clipboard {} image: {error:#}",
                format.key()
            );
            continue;
        }
        return Some(image);
    }
    None
}

fn cursor_name(style: CursorStyle) -> Option<&'static str> {
    Some(match style {
        CursorStyle::Arrow => "default",
        CursorStyle::IBeam => "text",
        CursorStyle::Crosshair => "crosshair",
        CursorStyle::ClosedHand => "grabbing",
        CursorStyle::OpenHand => "grab",
        CursorStyle::PointingHand => "pointer",
        CursorStyle::ResizeLeft => "w-resize",
        CursorStyle::ResizeRight => "e-resize",
        CursorStyle::ResizeLeftRight => "ew-resize",
        CursorStyle::ResizeUp => "n-resize",
        CursorStyle::ResizeDown => "s-resize",
        CursorStyle::ResizeUpDown => "ns-resize",
        CursorStyle::ResizeUpLeftDownRight => "nesw-resize",
        CursorStyle::ResizeUpRightDownLeft => "nwse-resize",
        CursorStyle::ResizeColumn => "col-resize",
        CursorStyle::ResizeRow => "row-resize",
        CursorStyle::IBeamCursorForVerticalLayout => "vertical-text",
        CursorStyle::OperationNotAllowed => "not-allowed",
        CursorStyle::DragLink => "alias",
        CursorStyle::DragCopy => "copy",
        CursorStyle::ContextualMenu => "context-menu",
        CursorStyle::None => "none",
    })
}

fn finite_extent(value: f32) -> i32 {
    if value.is_finite() {
        value.clamp(1.0, i32::MAX as f32) as i32
    } else {
        1
    }
}

fn valid_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn gtk_renderer_is_software(renderer_name: &str, renderer_override: Option<&str>) -> bool {
    let renderer = renderer_name.to_ascii_lowercase();
    let renderer_override = renderer_override.unwrap_or_default().to_ascii_lowercase();
    renderer.contains("cairo")
        || renderer.contains("llvmpipe")
        || renderer.contains("softpipe")
        || renderer.contains("software")
        || renderer.contains("swiftshader")
        || renderer_override.contains("cairo")
        || renderer_override.contains("llvmpipe")
        || renderer_override.contains("softpipe")
        || std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_some_and(|value| value != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_window_extent_rejects_hostile_values() {
        assert_eq!(finite_extent(f32::NAN), 1);
        assert_eq!(finite_extent(f32::INFINITY), 1);
        assert_eq!(finite_extent(-100.0), 1);
        assert_eq!(finite_extent(800.9), 800);
    }

    #[test]
    fn monitor_scale_is_never_zero_or_non_finite() {
        assert_eq!(valid_scale(0.0), 1.0);
        assert_eq!(valid_scale(f32::NAN), 1.0);
        assert_eq!(valid_scale(1.75), 1.75);
    }

    #[test]
    fn software_gsk_renderers_are_reported_honestly() {
        assert!(gtk_renderer_is_software("GskCairoRenderer", None));
        assert!(gtk_renderer_is_software("GskGLRenderer", Some("llvmpipe")));
        if std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_none() {
            assert!(!gtk_renderer_is_software("GskVulkanRenderer", None));
        }
    }

    #[test]
    fn frame_clock_sleeps_until_animation_or_pointer_lock_needs_it() {
        assert!(!frame_tick_required(false, PointerLockStatus::Unlocked));
        assert!(!frame_tick_required(false, PointerLockStatus::Unsupported));
        assert!(frame_tick_required(true, PointerLockStatus::Unlocked));
        assert!(frame_tick_required(false, PointerLockStatus::Requesting));
        assert!(frame_tick_required(false, PointerLockStatus::Locked));
    }

    #[test]
    fn xinput_raw_axes_follow_sparse_valuator_masks() {
        let values = [
            xinput::Fp3232 {
                integral: 12,
                frac: 0x8000_0000,
            },
            xinput::Fp3232 {
                integral: -4,
                frac: 0x4000_0000,
            },
            xinput::Fp3232 {
                integral: 99,
                frac: 0,
            },
        ];
        let mask = [0b101, 0b10];
        assert_eq!(xinput_raw_axis_values(&mask, &values, 0), 12.5);
        assert_eq!(xinput_raw_axis_values(&mask, &values, 1), 0.0);
        assert_eq!(xinput_raw_axis_values(&mask, &values, 2), -3.75);
        assert_eq!(xinput_raw_axis_values(&mask, &values, 33), 99.0);
        assert_eq!(xinput_raw_axis_values(&mask, &values, 65), 0.0);
    }
}
