use anyhow::{Context as _, anyhow};
use x11rb::connection::RequestConnection;

#[cfg(any())]
use crate::SharedString;
use crate::platform::blade::{BladeContext, BladeRenderer, BladeSurfaceConfig};
#[cfg(any())]
use crate::platform::linux::webview::{self as linux_webview, LinuxWebViewHost};
use crate::platform::tab_manager::{TabManagerState, WindowTabManager};
#[cfg(any())]
use crate::webview::{PlatformWebView, PlatformWebViewCommand};
use crate::{
    AnyWindowHandle, Bounds, Decorations, DevicePixels, DispatchEventResult, ForegroundExecutor,
    GameInputAvailability, GameInputCapabilities, GameInputError, GameInputErrorKind, GpuSpecs,
    LayerShellEdge, LayerShellOptions, Modifiers, Pixels, PlatformAtlas, PlatformDisplay,
    PlatformInput, PlatformInputHandler, PlatformWindow, Point, PointerLockStatus, PromptButton,
    PromptLevel, RequestFrameOptions, ResizeEdge, ScaledPixels, Scene, Size, Tiling,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowDecorations, WindowKind, WindowParams, X11ClientStatePtr, point, px, size,
};

use blade_graphics as gpu;
use raw_window_handle as rwh;
use util::{ResultExt, maybe};
use x11rb::{
    connection::Connection,
    cookie::{Cookie, VoidCookie},
    errors::ConnectionError,
    properties::WmSizeHints,
    protocol::{
        sync,
        xinput::{self, ConnectionExt as _},
        xproto::{self, ClientMessageEvent, ConnectionExt, TranslateCoordinatesReply},
    },
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
};

#[cfg(any())]
use std::collections::HashMap;
use std::{
    cell::RefCell,
    ffi::c_void,
    fmt::Display,
    num::NonZeroU32,
    ops::Div,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Mutex},
};

use super::{X11Display, XINPUT_ALL_DEVICE_GROUPS, XINPUT_ALL_DEVICES};

x11rb::atom_manager! {
    pub XcbAtoms: AtomsCookie {
        XA_ATOM,
        XdndAware,
        XdndStatus,
        XdndEnter,
        XdndLeave,
        XdndPosition,
        XdndSelection,
        XdndDrop,
        XdndFinished,
        XdndTypeList,
        XdndActionCopy,
        TextUriList: b"text/uri-list",
        UTF8_STRING,
        TEXT,
        STRING,
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        TEXT_PLAIN: b"text/plain",
        XDND_DATA,
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_CHANGE_STATE,
        WM_TRANSIENT_FOR,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_WM_STATE,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_FOCUSED,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_BELOW,
        _NET_WM_WINDOW_OPACITY,
        _NET_ACTIVE_WINDOW,
        _NET_WM_SYNC_REQUEST,
        _NET_WM_SYNC_REQUEST_COUNTER,
        _NET_WM_BYPASS_COMPOSITOR,
        _NET_WM_MOVERESIZE,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_SYNC,
        _NET_WM_STATE_DEMANDS_ATTENTION,
        _NET_SUPPORTED,
        _MOTIF_WM_HINTS,
        _GTK_SHOW_WINDOW_MENU,
        _GTK_FRAME_EXTENTS,
        _GTK_EDGE_CONSTRAINTS,
        _NET_CLIENT_LIST_STACKING,
    }
}

/// Computes an EWMH `_NET_WM_STRUT_PARTIAL` value (12 CARDINALs: left, right, top,
/// bottom, then a start/end pixel range along the perpendicular axis for each of
/// those four) for a `WindowKind::Top` window - X11's nearest equivalent to
/// wlr-layer-shell's exclusive zone. Returns `None` when the layer-shell options
/// don't request a reservation, matching that no space should be reserved from
/// the desktop's work area either.
fn net_wm_strut_partial(
    kind_options: LayerShellOptions,
    bounds: Bounds<DevicePixels>,
) -> Option<[u32; 12]> {
    let (zone, edge) = kind_options.exclusive_reservation()?;
    let reserve = f32::from(zone).max(0.0) as u32;
    let x0 = bounds.origin.x.0.max(0) as u32;
    let x1 = (bounds.origin.x.0 + bounds.size.width.0).max(0) as u32;
    let y0 = bounds.origin.y.0.max(0) as u32;
    let y1 = (bounds.origin.y.0 + bounds.size.height.0).max(0) as u32;

    let mut strut = [0u32; 12];
    match edge {
        LayerShellEdge::Top => {
            strut[2] = reserve;
            strut[8] = x0;
            strut[9] = x1;
        }
        LayerShellEdge::Bottom => {
            strut[3] = reserve;
            strut[10] = x0;
            strut[11] = x1;
        }
        LayerShellEdge::Left => {
            strut[0] = reserve;
            strut[4] = y0;
            strut[5] = y1;
        }
        LayerShellEdge::Right => {
            strut[1] = reserve;
            strut[6] = y0;
            strut[7] = y1;
        }
    }
    Some(strut)
}

fn query_render_extent(
    xcb: &Rc<XCBConnection>,
    x_window: xproto::Window,
) -> anyhow::Result<gpu::Extent> {
    let reply = get_reply(|| "X11 GetGeometry failed.", xcb.get_geometry(x_window))?;
    Ok(gpu::Extent {
        width: reply.width as u32,
        height: reply.height as u32,
        depth: 1,
    })
}

impl ResizeEdge {
    fn to_moveresize(self) -> u32 {
        match self {
            ResizeEdge::TopLeft => 0,
            ResizeEdge::Top => 1,
            ResizeEdge::TopRight => 2,
            ResizeEdge::Right => 3,
            ResizeEdge::BottomRight => 4,
            ResizeEdge::Bottom => 5,
            ResizeEdge::BottomLeft => 6,
            ResizeEdge::Left => 7,
        }
    }
}

#[derive(Debug)]
struct EdgeConstraints {
    top_tiled: bool,
    #[allow(dead_code)]
    top_resizable: bool,

    right_tiled: bool,
    #[allow(dead_code)]
    right_resizable: bool,

    bottom_tiled: bool,
    #[allow(dead_code)]
    bottom_resizable: bool,

    left_tiled: bool,
    #[allow(dead_code)]
    left_resizable: bool,
}

impl EdgeConstraints {
    fn from_atom(atom: u32) -> Self {
        EdgeConstraints {
            top_tiled: (atom & (1 << 0)) != 0,
            top_resizable: (atom & (1 << 1)) != 0,
            right_tiled: (atom & (1 << 2)) != 0,
            right_resizable: (atom & (1 << 3)) != 0,
            bottom_tiled: (atom & (1 << 4)) != 0,
            bottom_resizable: (atom & (1 << 5)) != 0,
            left_tiled: (atom & (1 << 6)) != 0,
            left_resizable: (atom & (1 << 7)) != 0,
        }
    }

    fn to_tiling(&self) -> Tiling {
        Tiling {
            top: self.top_tiled,
            right: self.right_tiled,
            bottom: self.bottom_tiled,
            left: self.left_tiled,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct Visual {
    id: xproto::Visualid,
    colormap: u32,
    depth: u8,
}

struct VisualSet {
    inherit: Visual,
    opaque: Option<Visual>,
    transparent: Option<Visual>,
    root: u32,
    black_pixel: u32,
}

fn find_visuals(xcb: &XCBConnection, screen_index: usize) -> VisualSet {
    let screen = &xcb.setup().roots[screen_index];
    let mut set = VisualSet {
        inherit: Visual {
            id: screen.root_visual,
            colormap: screen.default_colormap,
            depth: screen.root_depth,
        },
        opaque: None,
        transparent: None,
        root: screen.root,
        black_pixel: screen.black_pixel,
    };

    for depth_info in screen.allowed_depths.iter() {
        for visual_type in depth_info.visuals.iter() {
            let visual = Visual {
                id: visual_type.visual_id,
                colormap: 0,
                depth: depth_info.depth,
            };
            log::debug!(
                "Visual id: {}, class: {:?}, depth: {}, bits_per_value: {}, masks: 0x{:x} 0x{:x} 0x{:x}",
                visual_type.visual_id,
                visual_type.class,
                depth_info.depth,
                visual_type.bits_per_rgb_value,
                visual_type.red_mask,
                visual_type.green_mask,
                visual_type.blue_mask,
            );

            if (
                visual_type.red_mask,
                visual_type.green_mask,
                visual_type.blue_mask,
            ) != (0xFF0000, 0xFF00, 0xFF)
            {
                continue;
            }
            let color_mask = visual_type.red_mask | visual_type.green_mask | visual_type.blue_mask;
            let alpha_mask = color_mask as usize ^ ((1usize << depth_info.depth) - 1);

            if alpha_mask == 0 {
                if set.opaque.is_none() {
                    set.opaque = Some(visual);
                }
            } else {
                if set.transparent.is_none() {
                    set.transparent = Some(visual);
                }
            }
        }
    }

    set
}

struct RawWindow {
    connection: *mut c_void,
    screen_id: usize,
    window_id: u32,
    visual_id: u32,
}

#[derive(Default)]
pub struct Callbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> crate::DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    hovered_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
}

pub struct X11WindowState {
    pub destroyed: bool,
    client: X11ClientStatePtr,
    executor: ForegroundExecutor,
    atoms: XcbAtoms,
    x_root_window: xproto::Window,
    x_screen_index: usize,
    pub(crate) counter_id: sync::Counter,
    pub(crate) last_sync_counter: Option<sync::Int64>,
    bounds: Bounds<Pixels>,
    pub(crate) scale_factor: f32,
    renderer: BladeRenderer,
    display: Rc<dyn PlatformDisplay>,
    input_handler: Option<PlatformInputHandler>,
    appearance: WindowAppearance,
    background_appearance: WindowBackgroundAppearance,
    maximized_vertical: bool,
    maximized_horizontal: bool,
    hidden: bool,
    active: bool,
    hovered: bool,
    fullscreen: bool,
    client_side_decorations_supported: bool,
    decorations: WindowDecorations,
    edge_constraints: Option<EdgeConstraints>,
    pub handle: AnyWindowHandle,
    last_insets: [u32; 4],
    tab_manager: WindowTabManager,
    accessibility_root: crate::platform::linux::accessibility::AtSpiAccessibleRoot,
    pointer_lock: crate::game_input::NativePointerLockState,
    #[cfg(any())]
    pub(crate) webviews: HashMap<SharedString, LinuxWebViewHost>,
}

impl X11WindowState {
    fn is_transparent(&self) -> bool {
        self.background_appearance != WindowBackgroundAppearance::Opaque
    }
}

#[derive(Clone)]
pub(crate) struct X11WindowStatePtr {
    pub state: Rc<RefCell<X11WindowState>>,
    pub(crate) callbacks: Rc<RefCell<Callbacks>>,
    xcb: Rc<XCBConnection>,
    pub(crate) x_window: xproto::Window,
}

impl rwh::HasWindowHandle for RawWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let Some(non_zero) = NonZeroU32::new(self.window_id) else {
            log::error!("RawWindow.window_id zero when getting window handle.");
            return Err(rwh::HandleError::Unavailable);
        };
        let mut handle = rwh::XcbWindowHandle::new(non_zero);
        handle.visual_id = NonZeroU32::new(self.visual_id);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(handle.into()) })
    }
}
impl rwh::HasDisplayHandle for RawWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let Some(non_zero) = NonNull::new(self.connection) else {
            log::error!("Null RawWindow.connection when getting display handle.");
            return Err(rwh::HandleError::Unavailable);
        };
        let handle = rwh::XcbDisplayHandle::new(Some(non_zero), self.screen_id as i32);
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(handle.into()) })
    }
}

impl rwh::HasWindowHandle for X11Window {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let Some(non_zero) = NonZeroU32::new(self.0.x_window) else {
            return Err(rwh::HandleError::Unavailable);
        };
        let handle = rwh::XcbWindowHandle::new(non_zero);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(handle.into()) })
    }
}
impl rwh::HasDisplayHandle for X11Window {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let connection =
            as_raw_xcb_connection::AsRawXcbConnection::as_raw_xcb_connection(&*self.0.xcb)
                as *mut c_void;
        let Some(non_zero) = NonNull::new(connection) else {
            return Err(rwh::HandleError::Unavailable);
        };
        let screen_id = self.0.state.borrow().display.id().0 as i32;
        let handle = rwh::XcbDisplayHandle::new(Some(non_zero), screen_id);
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(handle.into()) })
    }
}

pub(crate) fn xcb_flush(xcb: &XCBConnection) {
    xcb.flush()
        .map_err(handle_connection_error)
        .context("X11 flush failed")
        .log_err();
}

pub(crate) fn check_reply<E, F, C>(
    failure_context: F,
    result: Result<VoidCookie<'_, C>, ConnectionError>,
) -> anyhow::Result<()>
where
    E: Display + Send + Sync + 'static,
    F: FnOnce() -> E,
    C: RequestConnection,
{
    result
        .map_err(handle_connection_error)
        .and_then(|response| response.check().map_err(|reply_error| anyhow!(reply_error)))
        .with_context(failure_context)
}

pub(crate) fn get_reply<E, F, C, O>(
    failure_context: F,
    result: Result<Cookie<'_, C, O>, ConnectionError>,
) -> anyhow::Result<O>
where
    E: Display + Send + Sync + 'static,
    F: FnOnce() -> E,
    C: RequestConnection,
    O: x11rb::x11_utils::TryParse,
{
    result
        .map_err(handle_connection_error)
        .and_then(|response| response.reply().map_err(|reply_error| anyhow!(reply_error)))
        .with_context(failure_context)
}

/// Convert X11 connection errors to `anyhow::Error` so backend failures can be
/// surfaced without aborting the application.
pub(crate) fn handle_connection_error(err: ConnectionError) -> anyhow::Error {
    match err {
        ConnectionError::UnknownError => anyhow!("X11 connection: Unknown error"),
        ConnectionError::UnsupportedExtension => anyhow!("X11 connection: Unsupported extension"),
        ConnectionError::MaximumRequestLengthExceeded => {
            anyhow!("X11 connection: Maximum request length exceeded")
        }
        ConnectionError::FdPassingFailed => {
            anyhow!("X11 connection: File descriptor passing failed")
        }
        ConnectionError::ParseError(parse_error) => {
            anyhow!(parse_error).context("Parse error in X11 response")
        }
        ConnectionError::InsufficientMemory => anyhow!("X11 connection: Insufficient memory"),
        ConnectionError::IoError(err) => anyhow!(err).context("X11 connection: IOError"),
        _ => anyhow!(err),
    }
}

impl X11WindowState {
    pub fn new(
        handle: AnyWindowHandle,
        client: X11ClientStatePtr,
        executor: ForegroundExecutor,
        gpu_context: &BladeContext,
        params: WindowParams,
        xcb: &Rc<XCBConnection>,
        client_side_decorations_supported: bool,
        x_main_screen_index: usize,
        x_window: xproto::Window,
        atoms: &XcbAtoms,
        scale_factor: f32,
        appearance: WindowAppearance,
        xinput_touch_supported: bool,
        parent_window: Option<xproto::Window>,
        tab_manager_state: Arc<Mutex<TabManagerState>>,
    ) -> anyhow::Result<Self> {
        let x_screen_index = params
            .display_id
            .map_or(x_main_screen_index, |did| did.0 as usize);

        let visual_set = find_visuals(xcb, x_screen_index);

        let visual = match visual_set.transparent {
            Some(visual) => visual,
            None => {
                log::warn!("Unable to find a transparent visual",);
                visual_set.inherit
            }
        };
        log::info!("Using {:?}", visual);

        let mut pointer_mask = xinput::XIEventMask::MOTION
            | xinput::XIEventMask::BUTTON_PRESS
            | xinput::XIEventMask::BUTTON_RELEASE
            | xinput::XIEventMask::ENTER
            | xinput::XIEventMask::LEAVE;
        if xinput_touch_supported {
            pointer_mask = pointer_mask
                | xinput::XIEventMask::TOUCH_BEGIN
                | xinput::XIEventMask::TOUCH_UPDATE
                | xinput::XIEventMask::TOUCH_END;
        }
        let colormap = if visual.colormap != 0 {
            visual.colormap
        } else {
            let id = xcb.generate_id()?;
            log::info!("Creating colormap {}", id);
            check_reply(
                || format!("X11 CreateColormap failed. id: {}", id),
                xcb.create_colormap(xproto::ColormapAlloc::NONE, id, visual_set.root, visual.id),
            )?;
            id
        };

        let win_aux = xproto::CreateWindowAux::new()
            // https://stackoverflow.com/questions/43218127/x11-xlib-xcb-creating-a-window-requires-border-pixel-if-specifying-colormap-wh
            .border_pixel(visual_set.black_pixel)
            .colormap(colormap)
            .event_mask(
                xproto::EventMask::EXPOSURE
                    | xproto::EventMask::STRUCTURE_NOTIFY
                    | xproto::EventMask::FOCUS_CHANGE
                    | xproto::EventMask::KEY_PRESS
                    | xproto::EventMask::KEY_RELEASE
                    | xproto::EventMask::PROPERTY_CHANGE
                    | xproto::EventMask::VISIBILITY_CHANGE,
            );

        let mut bounds = params.bounds.to_device_pixels(scale_factor);
        if bounds.size.width.0 == 0 || bounds.size.height.0 == 0 {
            log::warn!(
                "Window bounds contain a zero value. height={}, width={}. Falling back to defaults.",
                bounds.size.height.0,
                bounds.size.width.0
            );
            bounds.size.width = 800.into();
            bounds.size.height = 600.into();
        }

        check_reply(
            || {
                format!(
                    "X11 CreateWindow failed. depth: {}, x_window: {}, visual_set.root: {}, bounds.origin.x.0: {}, bounds.origin.y.0: {}, bounds.size.width.0: {}, bounds.size.height.0: {}",
                    visual.depth,
                    x_window,
                    visual_set.root,
                    bounds.origin.x.0 + 2,
                    bounds.origin.y.0,
                    bounds.size.width.0,
                    bounds.size.height.0
                )
            },
            xcb.create_window(
                visual.depth,
                x_window,
                visual_set.root,
                (bounds.origin.x.0 + 2) as i16,
                bounds.origin.y.0 as i16,
                bounds.size.width.0 as u16,
                bounds.size.height.0 as u16,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                visual.id,
                &win_aux,
            ),
        )?;

        // Collect errors during setup, so that window can be destroyed on failure.
        let setup_result = maybe!({
            let pid = std::process::id();
            check_reply(
                || "X11 ChangeProperty for _NET_WM_PID failed.",
                xcb.change_property32(
                    xproto::PropMode::REPLACE,
                    x_window,
                    atoms._NET_WM_PID,
                    xproto::AtomEnum::CARDINAL,
                    &[pid],
                ),
            )?;

            if let Some(size) = params.window_min_size {
                let mut size_hints = WmSizeHints::new();
                let min_size = (size.width.0 as i32, size.height.0 as i32);
                size_hints.min_size = Some(min_size);
                check_reply(
                    || {
                        format!(
                            "X11 change of WM_SIZE_HINTS failed. min_size: {:?}",
                            min_size
                        )
                    },
                    size_hints.set_normal_hints(xcb, x_window),
                )?;
            }

            let reply = get_reply(|| "X11 GetGeometry failed.", xcb.get_geometry(x_window))?;
            if reply.x == 0 && reply.y == 0 {
                bounds.origin.x.0 += 2;
                // Work around a bug where our rendered content appears
                // outside the window bounds when opened at the default position
                // (14px, 49px on X + Gnome + Ubuntu 22).
                let x = bounds.origin.x.0;
                let y = bounds.origin.y.0;
                check_reply(
                    || format!("X11 ConfigureWindow failed. x: {}, y: {}", x, y),
                    xcb.configure_window(x_window, &xproto::ConfigureWindowAux::new().x(x).y(y)),
                )?;
            }
            if let Some(titlebar) = params.titlebar
                && let Some(title) = titlebar.title
            {
                check_reply(
                    || "X11 ChangeProperty8 on window title failed.",
                    xcb.change_property8(
                        xproto::PropMode::REPLACE,
                        x_window,
                        xproto::AtomEnum::WM_NAME,
                        xproto::AtomEnum::STRING,
                        title.as_bytes(),
                    ),
                )?;
            }

            if params.kind == WindowKind::PopUp {
                check_reply(
                    || "X11 ChangeProperty32 setting window type for pop-up failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_NOTIFICATION],
                    ),
                )?;
            }

            if let Some(parent_window) = parent_window {
                // WM_TRANSIENT_FOR communicates the parent-child relationship to the window manager
                // so the child stays associated with, and stacked above, its parent.
                check_reply(
                    || "X11 ChangeProperty32 setting WM_TRANSIENT_FOR failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms.WM_TRANSIENT_FOR,
                        xproto::AtomEnum::WINDOW,
                        &[parent_window],
                    ),
                )?;
            }

            if params.kind == WindowKind::Floating {
                // _NET_WM_WINDOW_TYPE_DIALOG indicates that this is a dialog (floating) window
                // https://specifications.freedesktop.org/wm-spec/1.4/ar01s05.html
                check_reply(
                    || "X11 ChangeProperty32 setting window type for floating window failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_DIALOG],
                    ),
                )?;
            }

            if params.kind.is_overlay() {
                check_reply(
                    || "X11 ChangeProperty32 setting window type for overlay failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_DOCK],
                    ),
                )?;
                check_reply(
                    || "X11 ChangeProperty32 setting _NET_WM_STATE_ABOVE for overlay failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_STATE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_STATE_ABOVE],
                    ),
                )?;
            }

            // Layer-shell kinds with no wlr protocol on X11: approximated via EWMH
            // window-type/state hints, the closest X11 equivalents.
            if params.kind.is_wallpaper() {
                // _NET_WM_WINDOW_TYPE_DESKTOP tells the window manager this window is
                // the desktop itself, i.e. the wallpaper layer.
                check_reply(
                    || "X11 ChangeProperty32 setting window type for wallpaper failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_DESKTOP],
                    ),
                )?;
            }

            if params.kind.is_bottom() {
                // _NET_WM_STATE_BELOW asks the window manager to keep this window
                // beneath normal windows, X11's nearest equivalent to the bottom layer.
                check_reply(
                    || "X11 ChangeProperty32 setting _NET_WM_STATE_BELOW for bottom failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_STATE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_STATE_BELOW],
                    ),
                )?;
            }

            if let WindowKind::Top(kind_options) = params.kind {
                // _NET_WM_WINDOW_TYPE_DOCK marks this as a taskbar/panel-style window;
                // _NET_WM_STRUT[_PARTIAL] is the actual space reservation, X11's
                // equivalent to wlr-layer-shell's exclusive zone.
                check_reply(
                    || "X11 ChangeProperty32 setting window type for top failed.",
                    xcb.change_property32(
                        xproto::PropMode::REPLACE,
                        x_window,
                        atoms._NET_WM_WINDOW_TYPE,
                        xproto::AtomEnum::ATOM,
                        &[atoms._NET_WM_WINDOW_TYPE_DOCK],
                    ),
                )?;
                if let Some(strut) = net_wm_strut_partial(kind_options, bounds) {
                    check_reply(
                        || "X11 ChangeProperty32 setting _NET_WM_STRUT_PARTIAL for top failed.",
                        xcb.change_property32(
                            xproto::PropMode::REPLACE,
                            x_window,
                            atoms._NET_WM_STRUT_PARTIAL,
                            xproto::AtomEnum::CARDINAL,
                            &strut,
                        ),
                    )?;
                    // Older window managers only read the legacy 4-value _NET_WM_STRUT.
                    check_reply(
                        || "X11 ChangeProperty32 setting _NET_WM_STRUT for top failed.",
                        xcb.change_property32(
                            xproto::PropMode::REPLACE,
                            x_window,
                            atoms._NET_WM_STRUT,
                            xproto::AtomEnum::CARDINAL,
                            &strut[0..4],
                        ),
                    )?;
                }
            }

            if params.mouse_passthrough {
                use x11rb::protocol::shape;
                check_reply(
                    || "X11 shape::rectangles for mouse passthrough failed.",
                    shape::rectangles(
                        xcb.as_ref(),
                        shape::SO::SET,
                        shape::SK::INPUT,
                        xproto::ClipOrdering::UNSORTED,
                        x_window,
                        0,
                        0,
                        &[],
                    ),
                )?;
            }

            check_reply(
                || "X11 ChangeProperty32 setting protocols failed.",
                xcb.change_property32(
                    xproto::PropMode::REPLACE,
                    x_window,
                    atoms.WM_PROTOCOLS,
                    xproto::AtomEnum::ATOM,
                    &[atoms.WM_DELETE_WINDOW, atoms._NET_WM_SYNC_REQUEST],
                ),
            )?;

            get_reply(
                || "X11 sync protocol initialize failed.",
                sync::initialize(xcb, 3, 1),
            )?;
            let sync_request_counter = xcb.generate_id()?;
            check_reply(
                || "X11 sync CreateCounter failed.",
                sync::create_counter(xcb, sync_request_counter, sync::Int64 { lo: 0, hi: 0 }),
            )?;

            check_reply(
                || "X11 ChangeProperty32 setting sync request counter failed.",
                xcb.change_property32(
                    xproto::PropMode::REPLACE,
                    x_window,
                    atoms._NET_WM_SYNC_REQUEST_COUNTER,
                    xproto::AtomEnum::CARDINAL,
                    &[sync_request_counter],
                ),
            )?;

            check_reply(
                || "X11 XiSelectEvents failed.",
                xcb.xinput_xi_select_events(
                    x_window,
                    &[xinput::EventMask {
                        deviceid: XINPUT_ALL_DEVICE_GROUPS,
                        mask: vec![pointer_mask],
                    }],
                ),
            )?;

            check_reply(
                || "X11 XiSelectEvents for device changes failed.",
                xcb.xinput_xi_select_events(
                    x_window,
                    &[xinput::EventMask {
                        deviceid: XINPUT_ALL_DEVICES,
                        mask: vec![
                            xinput::XIEventMask::HIERARCHY | xinput::XIEventMask::DEVICE_CHANGED,
                        ],
                    }],
                ),
            )?;

            xcb_flush(xcb);

            let renderer = {
                let raw_window = RawWindow {
                    connection: as_raw_xcb_connection::AsRawXcbConnection::as_raw_xcb_connection(
                        xcb,
                    ) as *mut _,
                    screen_id: x_screen_index,
                    window_id: x_window,
                    visual_id: visual.id,
                };
                let config = BladeSurfaceConfig {
                    // Note: this has to be done after the GPU init, or otherwise
                    // the sizes are immediately invalidated.
                    size: query_render_extent(xcb, x_window)?,
                    // We set it to transparent by default, even if we have client-side
                    // decorations, since those seem to work on X11 even without `true` here.
                    // If the window appearance changes, then the renderer will get updated
                    // too
                    transparent: false,
                };
                BladeRenderer::new(gpu_context, &raw_window, config)?
            };

            let display = Rc::new(X11Display::new(xcb, scale_factor, x_screen_index)?);

            Ok(Self {
                client,
                executor,
                display,
                x_root_window: visual_set.root,
                x_screen_index,
                bounds: bounds.to_pixels(scale_factor),
                scale_factor,
                renderer,
                atoms: *atoms,
                input_handler: None,
                active: false,
                hovered: false,
                fullscreen: false,
                maximized_vertical: false,
                maximized_horizontal: false,
                hidden: false,
                appearance,
                handle,
                background_appearance: WindowBackgroundAppearance::Opaque,
                destroyed: false,
                client_side_decorations_supported,
                decorations: WindowDecorations::Server,
                last_insets: [0, 0, 0, 0],
                edge_constraints: None,
                counter_id: sync_request_counter,
                last_sync_counter: None,
                tab_manager: WindowTabManager::new(handle, tab_manager_state),
                accessibility_root: crate::platform::linux::accessibility::AtSpiAccessibleRoot::new(
                ),
                pointer_lock: crate::game_input::NativePointerLockState::new(true),
                #[cfg(any())]
                webviews: HashMap::default(),
            })
        });

        if setup_result.is_err() {
            check_reply(
                || "X11 DestroyWindow failed while cleaning it up after setup failure.",
                xcb.destroy_window(x_window),
            )?;
            xcb_flush(xcb);
        }

        setup_result
    }

    fn content_size(&self) -> Size<Pixels> {
        let size = self.renderer.viewport_size();
        Size {
            width: size.width.into(),
            height: size.height.into(),
        }
    }
}

pub(crate) struct X11Window(pub X11WindowStatePtr);

impl Drop for X11Window {
    fn drop(&mut self) {
        self.0.release_native_pointer_lock().log_err();
        let mut state = self.0.state.borrow_mut();

        // Clean up tab manager tracking for this window.
        state.tab_manager.remove_window();

        #[cfg(any())]
        state.webviews.clear();

        state.renderer.destroy();

        let destroy_x_window = maybe!({
            check_reply(
                || "X11 DestroyWindow failure.",
                self.0.xcb.destroy_window(self.0.x_window),
            )?;
            xcb_flush(&self.0.xcb);

            anyhow::Ok(())
        })
        .log_err();

        if destroy_x_window.is_some() {
            // Mark window as destroyed so that we can filter out when X11 events
            // for it still come in.
            state.destroyed = true;

            let this_ptr = self.0.clone();
            let client_ptr = state.client.clone();
            state
                .executor
                .spawn(async move {
                    this_ptr.close();
                    client_ptr.drop_window(this_ptr.x_window);
                })
                .detach();
        }

        drop(state);
    }
}

enum WmHintPropertyState {
    Remove = 0,
    Add = 1,
    Toggle = 2,
}

impl X11Window {
    pub fn new(
        handle: AnyWindowHandle,
        client: X11ClientStatePtr,
        executor: ForegroundExecutor,
        gpu_context: &BladeContext,
        params: WindowParams,
        xcb: &Rc<XCBConnection>,
        client_side_decorations_supported: bool,
        x_main_screen_index: usize,
        x_window: xproto::Window,
        atoms: &XcbAtoms,
        scale_factor: f32,
        appearance: WindowAppearance,
        xinput_touch_supported: bool,
        parent_window: Option<xproto::Window>,
        tab_manager_state: Arc<Mutex<TabManagerState>>,
    ) -> anyhow::Result<Self> {
        let ptr = X11WindowStatePtr {
            state: Rc::new(RefCell::new(X11WindowState::new(
                handle,
                client,
                executor,
                gpu_context,
                params,
                xcb,
                client_side_decorations_supported,
                x_main_screen_index,
                x_window,
                atoms,
                scale_factor,
                appearance,
                xinput_touch_supported,
                parent_window,
                tab_manager_state,
            )?)),
            callbacks: Rc::new(RefCell::new(Callbacks::default())),
            xcb: xcb.clone(),
            x_window,
        };

        let state = ptr.state.borrow_mut();
        ptr.set_wm_properties(state)?;

        Ok(Self(ptr))
    }

    fn set_wm_hints<C: Display + Send + Sync + 'static, F: FnOnce() -> C>(
        &self,
        failure_context: F,
        wm_hint_property_state: WmHintPropertyState,
        prop1: u32,
        prop2: u32,
    ) -> anyhow::Result<()> {
        let state = self.0.state.borrow();
        let message = ClientMessageEvent::new(
            32,
            self.0.x_window,
            state.atoms._NET_WM_STATE,
            [wm_hint_property_state as u32, prop1, prop2, 1, 0],
        );
        check_reply(
            failure_context,
            self.0.xcb.send_event(
                false,
                state.x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            ),
        )?;
        xcb_flush(&self.0.xcb);
        Ok(())
    }

    fn get_root_position(
        &self,
        position: Point<Pixels>,
    ) -> anyhow::Result<TranslateCoordinatesReply> {
        let state = self.0.state.borrow();
        get_reply(
            || "X11 TranslateCoordinates failed.",
            self.0.xcb.translate_coordinates(
                self.0.x_window,
                state.x_root_window,
                (position.x.0 * state.scale_factor) as i16,
                (position.y.0 * state.scale_factor) as i16,
            ),
        )
    }

    fn send_moveresize(&self, flag: u32) -> anyhow::Result<()> {
        let state = self.0.state.borrow();

        check_reply(
            || "X11 UngrabPointer before move/resize of window failed.",
            self.0.xcb.ungrab_pointer(x11rb::CURRENT_TIME),
        )?;

        let pointer = get_reply(
            || "X11 QueryPointer before move/resize of window failed.",
            self.0.xcb.query_pointer(self.0.x_window),
        )?;
        let message = ClientMessageEvent::new(
            32,
            self.0.x_window,
            state.atoms._NET_WM_MOVERESIZE,
            [
                pointer.root_x as u32,
                pointer.root_y as u32,
                flag,
                0, // Left mouse button
                0,
            ],
        );
        check_reply(
            || "X11 SendEvent to move/resize window failed.",
            self.0.xcb.send_event(
                false,
                state.x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            ),
        )?;

        xcb_flush(&self.0.xcb);
        Ok(())
    }
}

impl X11WindowStatePtr {
    pub(crate) fn pointer_lock_metrics(&self) -> (f32, Point<Pixels>) {
        let state = self.state.borrow();
        (
            state.scale_factor,
            point(
                px(state.bounds.size.width.0 * 0.5),
                px(state.bounds.size.height.0 * 0.5),
            ),
        )
    }

    fn request_native_pointer_lock(&self) -> Result<(), GameInputError> {
        let (client, active) = {
            let mut state = self.state.borrow_mut();
            if !state.pointer_lock.begin_request()? {
                return Ok(());
            }
            (state.client.clone(), state.active)
        };
        if !active {
            let error = GameInputError::new(
                GameInputErrorKind::Rejected,
                "X11 pointer lock requires the Kael window to be active",
            );
            return Err(self.state.borrow_mut().pointer_lock.fail(error));
        }

        match client.request_pointer_lock(self.x_window) {
            Ok(()) => {
                self.state.borrow_mut().pointer_lock.lock();
                Ok(())
            }
            Err(error) => Err(self.state.borrow_mut().pointer_lock.fail(error)),
        }
    }

    fn release_native_pointer_lock(&self) -> Result<(), GameInputError> {
        let client = self.state.borrow().client.clone();
        let result = client.release_pointer_lock(self.x_window);
        let mut state = self.state.borrow_mut();
        match result {
            Ok(()) => {
                state.pointer_lock.unlock();
                Ok(())
            }
            Err(error) => Err(state.pointer_lock.fail(error)),
        }
    }

    pub fn should_close(&self) -> bool {
        let callback = self.callbacks.borrow_mut().should_close.take();
        if let Some(mut should_close) = callback {
            let result = super::super::catch_platform_callback(
                "window should-close",
                false,
                &mut should_close,
            );
            self.callbacks.borrow_mut().should_close = Some(should_close);
            result
        } else {
            true
        }
    }

    pub fn property_notify(&self, event: xproto::PropertyNotifyEvent) -> anyhow::Result<()> {
        let mut state = self.state.borrow_mut();
        if event.atom == state.atoms._NET_WM_STATE {
            self.set_wm_properties(state)?;
        } else if event.atom == state.atoms._GTK_EDGE_CONSTRAINTS {
            self.set_edge_constraints(state)?;
        }
        Ok(())
    }

    fn set_edge_constraints(
        &self,
        mut state: std::cell::RefMut<X11WindowState>,
    ) -> anyhow::Result<()> {
        let reply = get_reply(
            || "X11 GetProperty for _GTK_EDGE_CONSTRAINTS failed.",
            self.xcb.get_property(
                false,
                self.x_window,
                state.atoms._GTK_EDGE_CONSTRAINTS,
                xproto::AtomEnum::CARDINAL,
                0,
                4,
            ),
        )?;

        if reply.value_len != 0 {
            if let Ok(bytes) = reply.value[0..4].try_into() {
                let atom = u32::from_ne_bytes(bytes);
                let edge_constraints = EdgeConstraints::from_atom(atom);
                state.edge_constraints.replace(edge_constraints);
            } else {
                log::error!("Failed to parse GTK_EDGE_CONSTRAINTS");
            }
        }

        Ok(())
    }

    fn set_wm_properties(
        &self,
        mut state: std::cell::RefMut<X11WindowState>,
    ) -> anyhow::Result<()> {
        let reply = get_reply(
            || "X11 GetProperty for _NET_WM_STATE failed.",
            self.xcb.get_property(
                false,
                self.x_window,
                state.atoms._NET_WM_STATE,
                xproto::AtomEnum::ATOM,
                0,
                u32::MAX,
            ),
        )?;

        let atoms = reply
            .value
            .chunks_exact(4)
            .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));

        state.active = false;
        state.fullscreen = false;
        state.maximized_vertical = false;
        state.maximized_horizontal = false;
        state.hidden = false;

        for atom in atoms {
            if atom == state.atoms._NET_WM_STATE_FOCUSED {
                state.active = true;
            } else if atom == state.atoms._NET_WM_STATE_FULLSCREEN {
                state.fullscreen = true;
            } else if atom == state.atoms._NET_WM_STATE_MAXIMIZED_VERT {
                state.maximized_vertical = true;
            } else if atom == state.atoms._NET_WM_STATE_MAXIMIZED_HORZ {
                state.maximized_horizontal = true;
            } else if atom == state.atoms._NET_WM_STATE_HIDDEN {
                state.hidden = true;
            }
        }

        Ok(())
    }

    pub fn close(&self) {
        let mut callbacks = self.callbacks.borrow_mut();
        if let Some(fun) = callbacks.close.take() {
            drop(callbacks);
            super::super::catch_platform_callback("window close", (), fun);
        }
    }

    pub fn refresh(&self, request_frame_options: RequestFrameOptions) {
        let mut callback = self.callbacks.borrow_mut().request_frame.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("frame request", (), || {
                fun(request_frame_options)
            });
        }
        self.callbacks.borrow_mut().request_frame = callback;
    }

    pub fn handle_input(&self, input: PlatformInput) {
        let mut callback = self.callbacks.borrow_mut().input.take();
        if let Some(ref mut fun) = callback {
            let result = super::super::catch_platform_callback(
                "window input",
                DispatchEventResult {
                    propagate: true,
                    default_prevented: false,
                },
                || fun(input.clone()),
            );
            self.callbacks.borrow_mut().input = callback;
            if !result.propagate {
                return;
            }
        }
        if let PlatformInput::KeyDown(event) = input {
            // only allow shift modifier when inserting text
            if event.keystroke.modifiers.is_subset_of(&Modifiers::shift()) {
                let mut state = self.state.borrow_mut();
                if let Some(mut input_handler) = state.input_handler.take() {
                    if let Some(key_char) = &event.keystroke.key_char {
                        drop(state);
                        input_handler.replace_text_in_range(None, key_char);
                        state = self.state.borrow_mut();
                    }
                    state.input_handler = Some(input_handler);
                }
            }
        }
    }

    pub fn handle_ime_commit(&self, text: String) {
        let mut state = self.state.borrow_mut();
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            input_handler.replace_text_in_range(None, &text);
            let mut state = self.state.borrow_mut();
            state.input_handler = Some(input_handler);
        }
    }

    pub fn handle_ime_preedit(&self, text: String) {
        let mut state = self.state.borrow_mut();
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            input_handler.replace_and_mark_text_in_range(None, &text, None);
            let mut state = self.state.borrow_mut();
            state.input_handler = Some(input_handler);
        }
    }

    pub fn handle_ime_unmark(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            input_handler.unmark_text();
            let mut state = self.state.borrow_mut();
            state.input_handler = Some(input_handler);
        }
    }

    pub fn handle_ime_delete(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            if let Some(marked) = input_handler.marked_text_range() {
                input_handler.replace_text_in_range(Some(marked), "");
            }
            let mut state = self.state.borrow_mut();
            state.input_handler = Some(input_handler);
        }
    }

    pub fn get_ime_area(&self) -> Option<Bounds<ScaledPixels>> {
        let mut state = self.state.borrow_mut();
        let scale_factor = state.scale_factor;
        let mut bounds: Option<Bounds<Pixels>> = None;
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            if let Some(selection) = input_handler.selected_text_range(true) {
                bounds = input_handler.bounds_for_range(selection.range);
            }
            let mut state = self.state.borrow_mut();
            state.input_handler = Some(input_handler);
        };
        bounds.map(|b| b.scale(scale_factor))
    }

    pub fn set_bounds(&self, bounds: Bounds<i32>) -> anyhow::Result<bool> {
        let root_origin = self
            .xcb
            .translate_coordinates(self.x_window, self.state.borrow().x_root_window, 0, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| Point {
                x: i32::from(reply.dst_x),
                y: i32::from(reply.dst_y),
            })
            .unwrap_or(bounds.origin);
        let physical_bounds = Bounds {
            origin: root_origin,
            size: bounds.size,
        };
        let (root, old_scale, old_display_id, screen_index) = {
            let state = self.state.borrow();
            (
                state.x_root_window,
                state.scale_factor,
                state.display.id(),
                state.x_screen_index,
            )
        };
        let new_display =
            X11Display::for_window(&self.xcb, root, screen_index, physical_bounds, old_scale).ok();
        let new_scale = new_display
            .as_ref()
            .map(PlatformDisplay::scale_factor)
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .unwrap_or(old_scale);
        let display_changed = new_display
            .as_ref()
            .is_some_and(|display| display.id() != old_display_id)
            || (new_scale - old_scale).abs() > f32::EPSILON;
        let (resize_args, is_move) = {
            let mut state = self.state.borrow_mut();
            if let Some(display) = new_display {
                state.display = Rc::new(display);
            }
            state.scale_factor = new_scale;
            let bounds = physical_bounds.map(|f| px(f as f32 / new_scale));

            let is_move = bounds.origin != state.bounds.origin;

            state.bounds = bounds;

            let gpu_size = query_render_extent(&self.xcb, self.x_window)?;
            state.renderer.update_drawable_size(size(
                DevicePixels(gpu_size.width as i32),
                DevicePixels(gpu_size.height as i32),
            ));
            let resize_args = (state.content_size(), state.scale_factor);
            if let Some(value) = state.last_sync_counter.take() {
                check_reply(
                    || "X11 sync SetCounter failed.",
                    sync::set_counter(&self.xcb, state.counter_id, value),
                )?;
            }
            (resize_args, is_move)
        };

        let (content_size, scale_factor) = resize_args;
        let mut callback = self.callbacks.borrow_mut().resize.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("window resize", (), || {
                fun(content_size, scale_factor)
            });
        }
        self.callbacks.borrow_mut().resize = callback;

        if is_move {
            let mut callback = self.callbacks.borrow_mut().moved.take();
            if let Some(ref mut fun) = callback {
                super::super::catch_platform_callback("window moved", (), fun);
            }
            self.callbacks.borrow_mut().moved = callback;
        }

        Ok(display_changed)
    }

    pub(crate) fn input_scale_factor(&self) -> f32 {
        self.state.borrow().scale_factor
    }

    pub fn set_active(&self, focus: bool) {
        self.state.borrow_mut().active = focus;
        if !focus {
            self.release_native_pointer_lock().log_err();
        }
        let mut callback = self.callbacks.borrow_mut().active_status_change.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("active status change", (), || fun(focus));
        }
        self.callbacks.borrow_mut().active_status_change = callback;
    }

    pub fn set_hovered(&self, focus: bool) {
        let mut callback = self.callbacks.borrow_mut().hovered_status_change.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("hover status change", (), || fun(focus));
        }
        self.callbacks.borrow_mut().hovered_status_change = callback;
    }

    pub fn set_appearance(&mut self, appearance: WindowAppearance) {
        let mut state = self.state.borrow_mut();
        state.appearance = appearance;
        let is_transparent = state.is_transparent();
        state.renderer.update_transparency(is_transparent);
        state.appearance = appearance;
        drop(state);
        self.notify_appearance_changed();
    }

    fn notify_appearance_changed(&self) {
        let mut callback = self.callbacks.borrow_mut().appearance_changed.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("appearance change", (), fun);
        }
        self.callbacks.borrow_mut().appearance_changed = callback;
    }
}

impl PlatformWindow for X11Window {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        let state = self.0.state.borrow();

        // A maximized window that gets minimized will still retain its maximized state.
        !state.hidden && state.maximized_vertical && state.maximized_horizontal
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.0.state.borrow();
        if self.is_maximized() {
            WindowBounds::Maximized(state.bounds)
        } else {
            WindowBounds::Windowed(state.bounds)
        }
    }

    fn inner_window_bounds(&self) -> WindowBounds {
        let state = self.0.state.borrow();
        if self.is_maximized() {
            WindowBounds::Maximized(state.bounds)
        } else {
            let mut bounds = state.bounds;
            let [left, right, top, bottom] = state.last_insets;

            let [left, right, top, bottom] = [
                Pixels((left as f32) / state.scale_factor),
                Pixels((right as f32) / state.scale_factor),
                Pixels((top as f32) / state.scale_factor),
                Pixels((bottom as f32) / state.scale_factor),
            ];

            bounds.origin.x += left;
            bounds.origin.y += top;
            bounds.size.width -= left + right;
            bounds.size.height -= top + bottom;

            WindowBounds::Windowed(bounds)
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        // We divide by the scale factor here because this value is queried to determine how much to draw,
        // but it will be multiplied later by the scale to adjust for scaling.
        let state = self.0.state.borrow();
        state
            .content_size()
            .map(|size| size.div(state.scale_factor))
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let state = self.0.state.borrow();
        let size = size.to_device_pixels(state.scale_factor);
        let width = crate::platform::safe_gpu_dimension(size.width.0 as f32);
        let height = crate::platform::safe_gpu_dimension(size.height.0 as f32);

        check_reply(
            || {
                format!(
                    "X11 ConfigureWindow failed. width: {}, height: {}",
                    width, height
                )
            },
            self.0.xcb.configure_window(
                self.0.x_window,
                &xproto::ConfigureWindowAux::new()
                    .width(width)
                    .height(height),
            ),
        )
        .log_err();
        xcb_flush(&self.0.xcb);
    }

    fn scale_factor(&self) -> f32 {
        self.0.state.borrow().scale_factor
    }

    fn appearance(&self) -> WindowAppearance {
        self.0.state.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.state.borrow().display.clone())
    }

    fn mouse_position(&self) -> Point<Pixels> {
        get_reply(
            || "X11 QueryPointer failed.",
            self.0.xcb.query_pointer(self.0.x_window),
        )
        .log_err()
        .map_or(Point::new(Pixels::ZERO, Pixels::ZERO), |reply| {
            Point::new((reply.root_x as u32).into(), (reply.root_y as u32).into())
        })
    }

    fn modifiers(&self) -> Modifiers {
        self.0
            .state
            .borrow()
            .client
            .0
            .upgrade()
            .map(|ref_cell| ref_cell.borrow().modifiers)
            .unwrap_or_default()
    }

    fn capslock(&self) -> crate::Capslock {
        self.0
            .state
            .borrow()
            .client
            .0
            .upgrade()
            .map(|ref_cell| ref_cell.borrow().capslock)
            .unwrap_or_default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self) {
        let data = [1, xproto::Time::CURRENT_TIME.into(), 0, 0, 0];
        let message = xproto::ClientMessageEvent::new(
            32,
            self.0.x_window,
            self.0.state.borrow().atoms._NET_ACTIVE_WINDOW,
            data,
        );
        self.0
            .xcb
            .send_event(
                false,
                self.0.state.borrow().x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            )
            .log_err();
        self.0
            .xcb
            .set_input_focus(
                xproto::InputFocus::POINTER_ROOT,
                self.0.x_window,
                xproto::Time::CURRENT_TIME,
            )
            .log_err();
        xcb_flush(&self.0.xcb);
    }

    fn is_active(&self) -> bool {
        self.0.state.borrow().active
    }

    fn is_hovered(&self) -> bool {
        self.0.state.borrow().hovered
    }

    fn set_title(&mut self, title: &str) {
        check_reply(
            || "X11 ChangeProperty8 on WM_NAME failed.",
            self.0.xcb.change_property8(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                xproto::AtomEnum::WM_NAME,
                xproto::AtomEnum::STRING,
                title.as_bytes(),
            ),
        )
        .log_err();

        check_reply(
            || "X11 ChangeProperty8 on _NET_WM_NAME failed.",
            self.0.xcb.change_property8(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                self.0.state.borrow().atoms._NET_WM_NAME,
                self.0.state.borrow().atoms.UTF8_STRING,
                title.as_bytes(),
            ),
        )
        .log_err();
        xcb_flush(&self.0.xcb);
        // Keep the tab manager's title in sync for tabbed_windows() results.
        self.0
            .state
            .borrow()
            .tab_manager
            .set_title(title.to_owned().into());
    }

    fn set_app_id(&mut self, app_id: &str) {
        let mut data = Vec::with_capacity(app_id.len() * 2 + 1);
        data.extend(app_id.bytes()); // instance https://unix.stackexchange.com/a/494170
        data.push(b'\0');
        data.extend(app_id.bytes()); // class

        check_reply(
            || "X11 ChangeProperty8 for WM_CLASS failed.",
            self.0.xcb.change_property8(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                xproto::AtomEnum::WM_CLASS,
                xproto::AtomEnum::STRING,
                &data,
            ),
        )
        .log_err();
    }

    fn map_window(&mut self) -> anyhow::Result<()> {
        check_reply(
            || "X11 MapWindow failed.",
            self.0.xcb.map_window(self.0.x_window),
        )?;
        // Waiting for the checked MapWindow reply can move MapNotify into XCB's
        // userspace queue. In that case the socket is no longer readable, so
        // calloop cannot discover the event through its file-descriptor source.
        // Drain the queue now so the initial retained frame can be scheduled.
        let client = self.0.state.borrow().client.clone();
        client.process_pending_x11_events();
        Ok(())
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        let mut state = self.0.state.borrow_mut();
        state.background_appearance = background_appearance;
        let transparent = state.is_transparent();
        state.renderer.update_transparency(transparent);
    }

    fn set_opacity(&self, opacity: f32) {
        let state = self.0.state.borrow();
        let opacity = (opacity.clamp(0.0, 1.0) * u32::MAX as f32).round() as u32;
        check_reply(
            || "X11 ChangeProperty for _NET_WM_WINDOW_OPACITY failed.",
            self.0.xcb.change_property32(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                state.atoms._NET_WM_WINDOW_OPACITY,
                xproto::AtomEnum::CARDINAL,
                &[opacity],
            ),
        )
        .log_err();
        xcb_flush(&self.0.xcb);
    }

    fn set_always_on_top(&self, always_on_top: bool) {
        let state = if always_on_top {
            WmHintPropertyState::Add
        } else {
            WmHintPropertyState::Remove
        };
        self.set_wm_hints(
            || "X11 SendEvent to update always-on-top state failed.",
            state,
            self.0.state.borrow().atoms._NET_WM_STATE_ABOVE,
            xproto::AtomEnum::NONE.into(),
        )
        .log_err();
    }

    fn minimize(&self) {
        let state = self.0.state.borrow();
        const WINDOW_ICONIC_STATE: u32 = 3;
        let message = ClientMessageEvent::new(
            32,
            self.0.x_window,
            state.atoms.WM_CHANGE_STATE,
            [WINDOW_ICONIC_STATE, 0, 0, 0, 0],
        );
        check_reply(
            || "X11 SendEvent to minimize window failed.",
            self.0.xcb.send_event(
                false,
                state.x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            ),
        )
        .log_err();
    }

    fn zoom(&self) {
        let state = self.0.state.borrow();
        self.set_wm_hints(
            || "X11 SendEvent to maximize a window failed.",
            WmHintPropertyState::Toggle,
            state.atoms._NET_WM_STATE_MAXIMIZED_VERT,
            state.atoms._NET_WM_STATE_MAXIMIZED_HORZ,
        )
        .log_err();
    }

    fn toggle_fullscreen(&self) {
        let state = self.0.state.borrow();
        self.set_wm_hints(
            || "X11 SendEvent to fullscreen a window failed.",
            WmHintPropertyState::Toggle,
            state.atoms._NET_WM_STATE_FULLSCREEN,
            xproto::AtomEnum::NONE.into(),
        )
        .log_err();
    }

    fn is_fullscreen(&self) -> bool {
        self.0.state.borrow().fullscreen
    }

    fn set_frame_polling(&self, active: bool) {
        self.0
            .state
            .borrow()
            .client
            .set_frame_polling(self.0.x_window, active);
    }

    fn close(&self) {
        if self.0.should_close() {
            self.0.close();
        }
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> crate::DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }

    fn game_input_capabilities(&self) -> GameInputCapabilities {
        let gamepads = if cfg!(feature = "game-input") {
            GameInputAvailability::Available
        } else {
            GameInputAvailability::DisabledAtCompileTime
        };
        GameInputCapabilities::new(GameInputAvailability::Available, gamepads)
    }

    fn pointer_lock_status(&self) -> PointerLockStatus {
        self.0.state.borrow().pointer_lock.status()
    }

    fn request_pointer_lock(&self) -> Result<(), GameInputError> {
        self.0.request_native_pointer_lock()
    }

    fn exit_pointer_lock(&self) -> Result<(), GameInputError> {
        self.0.release_native_pointer_lock()
    }

    fn pointer_lock_error(&self) -> Option<GameInputError> {
        self.0.state.borrow().pointer_lock.error()
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().hovered_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().appearance_changed = Some(callback);
    }

    #[cfg(any())]
    fn sync_webviews(&mut self, webviews: &[PlatformWebView]) {
        linux_webview::sync_x11_webviews(&self.0, webviews);
        let client = {
            let state = self.0.state.borrow();
            (!state.webviews.is_empty()).then(|| state.client.clone())
        };
        if let Some(client) = client {
            client.ensure_webview_event_pump();
        }
    }

    #[cfg(any())]
    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        linux_webview::dispatch_x11_webview_command(&self.0, command)
    }

    fn print(&mut self, job: crate::PlatformPrintJob) -> anyhow::Result<()> {
        crate::platform::linux::print::print_silent(job)
    }

    fn show_print_dialog(&mut self, job: crate::PlatformPrintJob) -> anyhow::Result<()> {
        let parent = ashpd::WindowIdentifier::from_xid(self.0.x_window.into());
        smol::block_on(crate::platform::linux::print::show_print_dialog(
            job,
            Some(parent),
        ))
    }

    fn export_scene_png(
        &self,
        scene: &Scene,
    ) -> std::result::Result<crate::Image, crate::WindowCaptureError> {
        let readback = self
            .0
            .state
            .borrow_mut()
            .renderer
            .render_scene_to_bgra(scene)
            .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))?;
        crate::platform::encode_bgra_png(
            readback.width,
            readback.height,
            readback.bgra,
            readback.premultiplied_alpha,
        )
        .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))
    }

    fn draw(&self, scene: &Scene) {
        let mut inner = self.0.state.borrow_mut();
        inner.renderer.draw(scene);
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        let inner = self.0.state.borrow();
        inner.renderer.sprite_atlas().clone()
    }

    fn show_window_menu(&self, position: Point<Pixels>) {
        self.0.release_native_pointer_lock().log_err();
        let state = self.0.state.borrow();

        check_reply(
            || "X11 UngrabPointer failed.",
            self.0.xcb.ungrab_pointer(x11rb::CURRENT_TIME),
        )
        .log_err();

        let Some(coords) = self.get_root_position(position).log_err() else {
            return;
        };
        let message = ClientMessageEvent::new(
            32,
            self.0.x_window,
            state.atoms._GTK_SHOW_WINDOW_MENU,
            [
                XINPUT_ALL_DEVICE_GROUPS as u32,
                coords.dst_x as u32,
                coords.dst_y as u32,
                0,
                0,
            ],
        );
        check_reply(
            || "X11 SendEvent to show window menu failed.",
            self.0.xcb.send_event(
                false,
                state.x_root_window,
                xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            ),
        )
        .log_err();
    }

    fn start_window_move(&self) {
        const MOVERESIZE_MOVE: u32 = 8;
        self.send_moveresize(MOVERESIZE_MOVE).log_err();
    }

    fn start_window_resize(&self, edge: ResizeEdge) {
        self.send_moveresize(edge.to_moveresize()).log_err();
    }

    fn window_decorations(&self) -> crate::Decorations {
        let state = self.0.state.borrow();

        // Client window decorations require compositor support
        if !state.client_side_decorations_supported {
            return Decorations::Server;
        }

        match state.decorations {
            WindowDecorations::Server => Decorations::Server,
            WindowDecorations::Client => {
                let tiling = if state.fullscreen {
                    Tiling::tiled()
                } else if let Some(edge_constraints) = &state.edge_constraints {
                    edge_constraints.to_tiling()
                } else {
                    // https://source.chromium.org/chromium/chromium/src/+/main:ui/ozone/platform/x11/x11_window.cc;l=2519;drc=1f14cc876cc5bf899d13284a12c451498219bb2d
                    Tiling {
                        top: state.maximized_vertical,
                        bottom: state.maximized_vertical,
                        left: state.maximized_horizontal,
                        right: state.maximized_horizontal,
                    }
                };
                Decorations::Client { tiling }
            }
        }
    }

    fn set_client_inset(&self, inset: Pixels) {
        let mut state = self.0.state.borrow_mut();

        let dp = (inset.0 * state.scale_factor) as u32;

        let insets = if state.fullscreen {
            [0, 0, 0, 0]
        } else if let Some(edge_constraints) = &state.edge_constraints {
            let left = if edge_constraints.left_tiled { 0 } else { dp };
            let top = if edge_constraints.top_tiled { 0 } else { dp };
            let right = if edge_constraints.right_tiled { 0 } else { dp };
            let bottom = if edge_constraints.bottom_tiled { 0 } else { dp };

            [left, right, top, bottom]
        } else {
            let (left, right) = if state.maximized_horizontal {
                (0, 0)
            } else {
                (dp, dp)
            };
            let (top, bottom) = if state.maximized_vertical {
                (0, 0)
            } else {
                (dp, dp)
            };
            [left, right, top, bottom]
        };

        if state.last_insets != insets {
            state.last_insets = insets;

            check_reply(
                || "X11 ChangeProperty for _GTK_FRAME_EXTENTS failed.",
                self.0.xcb.change_property(
                    xproto::PropMode::REPLACE,
                    self.0.x_window,
                    state.atoms._GTK_FRAME_EXTENTS,
                    xproto::AtomEnum::CARDINAL,
                    size_of::<u32>() as u8 * 8,
                    4,
                    bytemuck::cast_slice::<u32, u8>(&insets),
                ),
            )
            .log_err();
        }
    }

    fn request_decorations(&self, mut decorations: crate::WindowDecorations) {
        let mut state = self.0.state.borrow_mut();

        if matches!(decorations, crate::WindowDecorations::Client)
            && !state.client_side_decorations_supported
        {
            log::info!(
                "x11: no compositor present, falling back to server-side window decorations"
            );
            decorations = crate::WindowDecorations::Server;
        }

        // https://github.com/rust-windowing/winit/blob/master/src/platform_impl/linux/x11/util/hint.rs#L53-L87
        let hints_data: [u32; 5] = match decorations {
            WindowDecorations::Server => [1 << 1, 0, 1, 0, 0],
            WindowDecorations::Client => [1 << 1, 0, 0, 0, 0],
        };

        let success = check_reply(
            || "X11 ChangeProperty for _MOTIF_WM_HINTS failed.",
            self.0.xcb.change_property(
                xproto::PropMode::REPLACE,
                self.0.x_window,
                state.atoms._MOTIF_WM_HINTS,
                state.atoms._MOTIF_WM_HINTS,
                size_of::<u32>() as u8 * 8,
                5,
                bytemuck::cast_slice::<u32, u8>(&hints_data),
            ),
        )
        .log_err();

        let Some(()) = success else {
            return;
        };

        match decorations {
            WindowDecorations::Server => {
                state.decorations = WindowDecorations::Server;
                let is_transparent = state.is_transparent();
                state.renderer.update_transparency(is_transparent);
            }
            WindowDecorations::Client => {
                state.decorations = WindowDecorations::Client;
                let is_transparent = state.is_transparent();
                state.renderer.update_transparency(is_transparent);
            }
        }

        drop(state);
        self.0.notify_appearance_changed();
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let mut state = self.0.state.borrow_mut();
        let client = state.client.clone();
        drop(state);
        client.update_ime_position(bounds);
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.0.state.borrow().renderer.gpu_specs().into()
    }

    fn show(&self) {
        self.0.xcb.map_window(self.0.x_window).log_err();
        xcb_flush(&self.0.xcb);
        self.0.state.borrow_mut().hidden = false;
    }

    fn hide(&self) {
        self.0.xcb.unmap_window(self.0.x_window).log_err();
        xcb_flush(&self.0.xcb);
        self.0.state.borrow_mut().hidden = true;
    }

    fn is_visible(&self) -> bool {
        !self.0.state.borrow().hidden
    }

    fn show_character_palette(&self) {
        super::super::character_palette::show_character_palette();
    }

    fn set_mouse_passthrough(&self, passthrough: bool) {
        use x11rb::protocol::shape;
        if passthrough {
            shape::rectangles(
                self.0.xcb.as_ref(),
                shape::SO::SET,
                shape::SK::INPUT,
                xproto::ClipOrdering::UNSORTED,
                self.0.x_window,
                0,
                0,
                &[],
            )
            .log_err();
        } else {
            shape::mask(
                self.0.xcb.as_ref(),
                shape::SO::SET,
                shape::SK::INPUT,
                self.0.x_window,
                0,
                0,
                x11rb::NONE,
            )
            .log_err();
        }
        xcb_flush(&self.0.xcb);
    }

    fn set_tabbing_identifier(&self, identifier: Option<String>) {
        self.0
            .state
            .borrow()
            .tab_manager
            .set_tabbing_identifier(identifier);
    }

    fn merge_all_windows(&self) {
        self.0.state.borrow().tab_manager.merge_all_windows();
    }

    fn move_tab_to_new_window(&self) {
        self.0.state.borrow().tab_manager.move_tab_to_new_window();
    }

    fn tabbed_windows(&self) -> Option<Vec<crate::SystemWindowTab>> {
        self.0.state.borrow().tab_manager.tabbed_windows()
    }

    fn display_refresh_rate(&self) -> Option<f32> {
        self.0.state.borrow().display.refresh_rate()
    }

    fn update_accessibility_tree(
        &mut self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        let state = self.0.state.borrow();
        state.accessibility_root.update_tree(tree);
        state.accessibility_root.drain_actions(tree)
    }
}
