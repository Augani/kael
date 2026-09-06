use std::{
    cell::{Ref, RefCell, RefMut},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, Mutex},
};

use blade_graphics as gpu;
use collections::HashMap;
use futures::channel::oneshot::Receiver;

use raw_window_handle as rwh;
use util::shell;
use wayland_backend::client::ObjectId;
use wayland_client::WEnum;
use wayland_client::{Proxy, protocol::wl_surface};
use wayland_protocols::wp::viewporter::client::wp_viewport;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1;
use wayland_protocols::xdg::shell::client::xdg_surface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self};
use wayland_protocols::{
    wp::fractional_scale::v1::client::wp_fractional_scale_v1,
    wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1,
    wp::relative_pointer::zv1::client::zwp_relative_pointer_v1,
    xdg::shell::client::xdg_toplevel::XdgToplevel,
};
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur;
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

#[cfg(any())]
use crate::platform::linux::webview as linux_webview;
use crate::platform::tab_manager::{TabManagerState, WindowTabManager};
#[cfg(any())]
use crate::webview::{PlatformWebView, PlatformWebViewCommand};
use crate::{
    AnyWindowHandle, Bounds, Decorations, DispatchEventResult, GameInputAvailability,
    GameInputCapabilities, GameInputError, GameInputErrorKind, Globals, GpuSpecs, Modifiers,
    Output, Pixels, PlatformDisplay, PlatformInput, Point, PointerLockStatus, PromptButton,
    PromptLevel, RequestFrameOptions, ResizeEdge, Size, Tiling, WaylandClientStatePtr,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowControls,
    WindowDecorations, WindowParams, point, px, size,
};
use crate::{
    Capslock,
    platform::{
        PlatformAtlas, PlatformInputHandler, PlatformWindow,
        blade::{BladeContext, BladeRenderer, BladeSurfaceConfig},
        linux::wayland::{display::WaylandDisplay, serial::SerialKind},
    },
};
use crate::{WindowKind, scene::Scene};

#[derive(Default)]
pub(crate) struct Callbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(crate::PlatformInput) -> crate::DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    hover_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
}

struct RawWindow {
    window: *mut c_void,
    display: *mut c_void,
}

impl rwh::HasWindowHandle for RawWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let window = NonNull::new(self.window).ok_or(rwh::HandleError::Unavailable)?;
        let handle = rwh::WaylandWindowHandle::new(window);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(handle.into()) })
    }
}
impl rwh::HasDisplayHandle for RawWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let display = NonNull::new(self.display).ok_or(rwh::HandleError::Unavailable)?;
        let handle = rwh::WaylandDisplayHandle::new(display);
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(handle.into()) })
    }
}

#[derive(Debug)]
struct InProgressConfigure {
    size: Option<Size<Pixels>>,
    fullscreen: bool,
    maximized: bool,
    resizing: bool,
    tiling: Tiling,
}

pub struct WaylandWindowState {
    /// `None` for [`WindowKind::Overlay`] windows backed by a wlr-layer-shell surface,
    /// whose role is a layer surface rather than an xdg-toplevel.
    xdg_surface: Option<xdg_surface::XdgSurface>,
    /// Present only for overlay windows on compositors that implement wlr-layer-shell.
    /// Mutually exclusive with `xdg_surface`/`toplevel`: a wl_surface may hold only one role.
    layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    acknowledged_first_configure: bool,
    frame_callback_active: bool,
    frame_callback_requested: bool,
    /// Forces the next `frame()` call to render past the `frame_callback_active` gate,
    /// regardless of `invalidator` dirty state. Set for a wlr-layer-shell surface's
    /// mandatory first paint after `ack_configure` (frame_callback_active can't be
    /// trusted at that point - see `handle_layer_surface_event`), and reused whenever
    /// a window created with `show: false` shows its deferred first paint via `show()`.
    force_next_frame: bool,
    pub surface: wl_surface::WlSurface,
    decoration: Option<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
    app_id: Option<String>,
    appearance: WindowAppearance,
    blur: Option<org_kde_kwin_blur::OrgKdeKwinBlur>,
    /// `None` for overlay windows backed by a layer surface.
    toplevel: Option<xdg_toplevel::XdgToplevel>,
    viewport: Option<wp_viewport::WpViewport>,
    outputs: HashMap<ObjectId, Output>,
    display: Option<(ObjectId, Output)>,
    globals: Globals,
    renderer: BladeRenderer,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) scale: f32,
    input_handler: Option<PlatformInputHandler>,
    decorations: WindowDecorations,
    background_appearance: WindowBackgroundAppearance,
    fullscreen: bool,
    maximized: bool,
    tiling: Tiling,
    window_bounds: Bounds<Pixels>,
    client: WaylandClientStatePtr,
    handle: AnyWindowHandle,
    active: bool,
    hovered: bool,
    in_progress_configure: Option<InProgressConfigure>,
    resize_throttle: bool,
    in_progress_window_controls: Option<WindowControls>,
    window_controls: WindowControls,
    client_inset: Option<Pixels>,
    visible: bool,
    tab_manager: WindowTabManager,
    accessibility_root: crate::platform::linux::accessibility::AtSpiAccessibleRoot,
    pointer_lock: crate::game_input::NativePointerLockState,
    locked_pointer: Option<zwp_locked_pointer_v1::ZwpLockedPointerV1>,
    relative_pointer: Option<zwp_relative_pointer_v1::ZwpRelativePointerV1>,
}

#[derive(Clone)]
pub struct WaylandWindowStatePtr {
    pub(crate) state: Rc<RefCell<WaylandWindowState>>,
    callbacks: Rc<RefCell<Callbacks>>,
}

impl WaylandWindowState {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        handle: AnyWindowHandle,
        surface: wl_surface::WlSurface,
        xdg_surface: Option<xdg_surface::XdgSurface>,
        toplevel: Option<xdg_toplevel::XdgToplevel>,
        layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
        decoration: Option<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
        appearance: WindowAppearance,
        viewport: Option<wp_viewport::WpViewport>,
        client: WaylandClientStatePtr,
        globals: Globals,
        gpu_context: &BladeContext,
        options: WindowParams,
        tab_manager_state: Arc<Mutex<TabManagerState>>,
    ) -> anyhow::Result<Self> {
        let pointer_lock_supported =
            globals.pointer_constraints.is_some() && globals.relative_pointer_manager.is_some();
        let renderer = {
            let backend = surface.backend().upgrade().ok_or_else(|| {
                anyhow::anyhow!("Wayland display backend disappeared while creating a window")
            })?;
            let raw_window = RawWindow {
                window: surface.id().as_ptr().cast::<c_void>(),
                display: backend.display_ptr().cast::<c_void>(),
            };
            let config = BladeSurfaceConfig {
                size: gpu::Extent {
                    width: crate::platform::safe_gpu_dimension(options.bounds.size.width.0),
                    height: crate::platform::safe_gpu_dimension(options.bounds.size.height.0),
                    depth: 1,
                },
                transparent: true,
            };
            BladeRenderer::new(gpu_context, &raw_window, config)?
        };

        Ok(Self {
            xdg_surface,
            layer_surface,
            acknowledged_first_configure: false,
            frame_callback_active: false,
            frame_callback_requested: false,
            force_next_frame: false,
            surface,
            decoration,
            app_id: None,
            blur: None,
            toplevel,
            viewport,
            globals,
            outputs: HashMap::default(),
            display: None,
            renderer,
            bounds: options.bounds,
            scale: 1.0,
            input_handler: None,
            decorations: WindowDecorations::Client,
            background_appearance: WindowBackgroundAppearance::Opaque,
            fullscreen: false,
            maximized: false,
            tiling: Tiling::default(),
            window_bounds: options.bounds,
            in_progress_configure: None,
            resize_throttle: false,
            client,
            appearance,
            handle,
            active: false,
            hovered: false,
            in_progress_window_controls: None,
            window_controls: WindowControls::default(),
            client_inset: None,
            visible: options.show,
            tab_manager: WindowTabManager::new(handle, tab_manager_state),
            accessibility_root: crate::platform::linux::accessibility::AtSpiAccessibleRoot::new(),
            pointer_lock: crate::game_input::NativePointerLockState::new(pointer_lock_supported),
            locked_pointer: None,
            relative_pointer: None,
        })
    }

    pub fn is_transparent(&self) -> bool {
        self.decorations == WindowDecorations::Client
            || self.background_appearance != WindowBackgroundAppearance::Opaque
    }

    pub fn primary_output_scale(&mut self) -> i32 {
        let mut scale = 1;
        let mut current_output = self.display.take();
        for (id, output) in self.outputs.iter() {
            if let Some((_, output_data)) = &current_output {
                if output.scale > output_data.scale {
                    current_output = Some((id.clone(), output.clone()));
                }
            } else {
                current_output = Some((id.clone(), output.clone()));
            }
            scale = scale.max(output.scale);
        }
        self.display = current_output;
        scale
    }

    pub fn inset(&self) -> Pixels {
        match self.decorations {
            WindowDecorations::Server => px(0.0),
            WindowDecorations::Client => self.client_inset.unwrap_or(px(0.0)),
        }
    }

    /// Returns the refresh rate in Hz of the primary output this window is on.
    pub fn display_refresh_rate(&self) -> Option<f32> {
        // Try the current display first
        if let Some((_, output)) = &self.display {
            if let Some(mhz) = output.refresh_mhz {
                if mhz > 0 {
                    return Some(mhz as f32 / 1000.0);
                }
            }
        }
        // Fall back to any output the window is on
        for (_, output) in &self.outputs {
            if let Some(mhz) = output.refresh_mhz {
                if mhz > 0 {
                    return Some(mhz as f32 / 1000.0);
                }
            }
        }
        None
    }
}

pub(crate) struct WaylandWindow(pub WaylandWindowStatePtr);
pub enum ImeInput {
    InsertText(String),
    SetMarkedText(String),
    UnmarkText,
    DeleteText,
}

impl Drop for WaylandWindow {
    fn drop(&mut self) {
        self.0.release_native_pointer_lock().ok();
        let mut state = self.0.state.borrow_mut();
        let surface_id = state.surface.id();
        let client = state.client.clone();

        // Clean up tab manager tracking for this window.
        state.tab_manager.remove_window();

        state.renderer.destroy();
        if let Some(decoration) = &state.decoration {
            decoration.destroy();
        }
        if let Some(blur) = &state.blur {
            blur.release();
        }
        if let Some(toplevel) = &state.toplevel {
            toplevel.destroy();
        }
        if let Some(viewport) = &state.viewport {
            viewport.destroy();
        }
        if let Some(layer_surface) = &state.layer_surface {
            layer_surface.destroy();
        }
        if let Some(xdg_surface) = &state.xdg_surface {
            xdg_surface.destroy();
        }
        state.surface.destroy();

        let state_ptr = self.0.clone();
        state
            .globals
            .executor
            .spawn(async move {
                state_ptr.close();
                client.drop_window(&surface_id)
            })
            .detach();
        drop(state);
    }
}

impl WaylandWindow {
    fn borrow(&self) -> Ref<'_, WaylandWindowState> {
        self.0.state.borrow()
    }

    fn borrow_mut(&self) -> RefMut<'_, WaylandWindowState> {
        self.0.state.borrow_mut()
    }

    fn set_exclusive_edge(
        edge: crate::Anchor,
        anchor: crate::Anchor,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    ) {
        if edge.bits().count_ones() == 1 && anchor.contains(edge) {
            layer_surface.set_exclusive_edge(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
                edge.bits(),
            ));
        }
    }

    pub fn new(
        handle: AnyWindowHandle,
        globals: Globals,
        gpu_context: &BladeContext,
        client: WaylandClientStatePtr,
        params: WindowParams,
        appearance: WindowAppearance,
        parent: Option<XdgToplevel>,
        tab_manager_state: Arc<Mutex<TabManagerState>>,
    ) -> anyhow::Result<(Self, ObjectId)> {
        let surface = globals.compositor.create_surface(&globals.qh, ());

        // An overlay window is given a wlr-layer-shell surface on the overlay layer so it
        // renders above all other surfaces, including fullscreen ones. The role is mutually
        // exclusive with xdg-toplevel, so we create one or the other for a given wl_surface.
        let (use_layer_shell,kind_options,layer,namespace) = match params.kind {
            WindowKind::Overlay(kind_options) => {
                (true && globals.layer_shell.is_some(),Some(kind_options),Some(zwlr_layer_shell_v1::Layer::Overlay),"kael-overlay")
            }
            WindowKind::Top(kind_options) => {
                (true && globals.layer_shell.is_some(),Some(kind_options),Some(zwlr_layer_shell_v1::Layer::Top),"kael-top")
            }
            WindowKind::Bottom(kind_options) => {
                (true && globals.layer_shell.is_some(),Some(kind_options),Some(zwlr_layer_shell_v1::Layer::Bottom),"kael-bottom")
            }
            WindowKind::Background(kind_options) => {
                (true && globals.layer_shell.is_some(),Some(kind_options),Some(zwlr_layer_shell_v1::Layer::Background),"kael-background")
            }   
            _ => (false, None, None, ""),
        };

        let (xdg_surface, toplevel, layer_surface, decoration) = if use_layer_shell {
            let Some(layer_shell) = globals.layer_shell.as_ref() else {
                anyhow::bail!("Wayland layer-shell disappeared during window creation");
            };
            let layer_surface = layer_shell.get_layer_surface(
                &surface,
                None,
                layer.unwrap(),
                namespace.to_owned(),
                &globals.qh,
                surface.id(),
            );
            // Defaults preserve the previous overlay behaviour: a free-floating, explicitly
            // sized surface that does not reserve screen space. Anchoring is left empty so the
            // compositor positions the surface, and the exclusive zone is 0 (no reservation).
            let dp_size = params
                .bounds
                .size
                .to_device_pixels(1.0)
                .map(|value| value.0.max(1) as u32);
            layer_surface.set_size(dp_size.width, dp_size.height);

            if let Some(shell_options) = kind_options { 
                layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
                    shell_options.anchor.bits(),
                ));
                if layer_surface.version() >= 5 {
                    if let Some(exc_edge) = shell_options.exclusive_edge {
                        Self::set_exclusive_edge(exc_edge, shell_options.anchor,&layer_surface);
                    }
                }
                if let Some(exc_zone) = shell_options.exclusive_zone {
                    layer_surface.set_exclusive_zone(f32::from(exc_zone) as i32);
                }

                if let Some((top, rigth, bottom, left)) = shell_options.margin {
                    layer_surface.set_margin(
                        f32::from(top) as i32,
                        f32::from(rigth) as i32,
                        f32::from(bottom) as i32,
                        f32::from(left) as i32, 
                    );
                }
                if layer_surface.version() >= 4 {
                    // `focus: false` means the caller doesn't want this surface eligible
                    // for keyboard focus on map. wlr-layer-shell has no separate "don't
                    // focus me" hint, so the only lever is refusing keyboard interactivity
                    // outright, overriding whatever `WindowKindOptions` requested.
                    let keyboard_interactivity = if params.focus {
                        shell_options.keyboard_interactivity
                    } else {
                        crate::KeyboardInteractivity::None
                    };
                    match keyboard_interactivity {
                        crate::KeyboardInteractivity::OnDemand => layer_surface
                            .set_keyboard_interactivity(
                                zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
                            ),
                        crate::KeyboardInteractivity::None => layer_surface
                            .set_keyboard_interactivity(
                                zwlr_layer_surface_v1::KeyboardInteractivity::None,
                            ),
                        crate::KeyboardInteractivity::Exclusive => layer_surface
                            .set_keyboard_interactivity(
                                zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
                            ),
                    }
                }
            }

            (None, None, Some(layer_surface), None)
        } else {
            let xdg_surface = globals
                .wm_base
                .get_xdg_surface(&surface, &globals.qh, surface.id());
            let toplevel = xdg_surface.get_toplevel(&globals.qh, surface.id());

            if let Some(parent) = parent.as_ref() {
                toplevel.set_parent(Some(parent));
            }

            if let WindowKind::Overlay(_) = params.kind {
                log::warn!(
                    "Wayland: WindowKind::Overlay requested but the compositor does not \
                     implement wlr-layer-shell; falling back to a regular window. True \
                     always-on-top (above fullscreen) is unavailable on this compositor."
                );
            }

            if let WindowKind::Top(_) = params.kind {
                log::warn!("Wayland: WindowKind::Top requested but the compositor does not \
                     implement wlr-layer-shell; falling back to a regular window. True \
                     always-on-top (above fullscreen) is unavailable on this compositor."
                );

            }
            if let WindowKind::Bottom(_) = params.kind {
                log::warn!("Wayland: WindowKind::Bottom requested but the compositor does not \
                     implement wlr-layer-shell; falling back to a regular window. True \
                     behind other windows is unavailable on this compositor."
                );

            }
                    
            if let WindowKind::Background(_) = params.kind {
                log::warn!("Wayland: WindowKind::Background requested but the compositor does not \
                     implement wlr-layer-shell; falling back to a regular window. True \
                     behid all other windows and widgets is unavailable on this compositor."
                );

            }
                    
            if let Some(size) = params.window_min_size {
                toplevel.set_min_size(size.width.0 as i32, size.height.0 as i32);
            }
            // Attempt to set up window decorations based on the requested configuration
            let decoration = globals
                .decoration_manager
                .as_ref()
                .map(|decoration_manager| {
                    decoration_manager.get_toplevel_decoration(&toplevel, &globals.qh, surface.id())
                });

            (Some(xdg_surface), Some(toplevel), None, decoration)
        };

        if let Some(fractional_scale_manager) = globals.fractional_scale_manager.as_ref() {
            fractional_scale_manager.get_fractional_scale(&surface, &globals.qh, surface.id());
        }

        let viewport = globals
            .viewporter
            .as_ref()
            .map(|viewporter| viewporter.get_viewport(&surface, &globals.qh, ()));

        let mouse_passthrough = params.mouse_passthrough;

        let this = Self(WaylandWindowStatePtr {
            state: Rc::new(RefCell::new(WaylandWindowState::new(
                handle,
                surface.clone(),
                xdg_surface,
                toplevel,
                layer_surface,
                decoration,
                appearance,
                viewport,
                client,
                globals,
                gpu_context,
                params,
                tab_manager_state,
            )?)),
            callbacks: Rc::new(RefCell::new(Callbacks::default())),
        });

        if mouse_passthrough {
            this.set_mouse_passthrough(true);
        }

        // Kick things off
        surface.commit();

        Ok((this, surface.id()))
    }
}

impl WaylandWindowStatePtr {
    pub fn handle(&self) -> AnyWindowHandle {
        self.state.borrow().handle
    }

    pub fn surface(&self) -> wl_surface::WlSurface {
        self.state.borrow().surface.clone()
    }

    pub fn toplevel(&self) -> Option<xdg_toplevel::XdgToplevel> {
        self.state.borrow().toplevel.clone()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
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
                "Wayland pointer lock requires the Kael window to be active",
            );
            return Err(self.state.borrow_mut().pointer_lock.fail(error));
        }

        match client.request_pointer_lock(self) {
            Ok((locked_pointer, relative_pointer)) => {
                let mut state = self.state.borrow_mut();
                state.locked_pointer = Some(locked_pointer);
                state.relative_pointer = Some(relative_pointer);
                Ok(())
            }
            Err(error) => Err(self.state.borrow_mut().pointer_lock.fail(error)),
        }
    }

    pub(crate) fn release_native_pointer_lock(&self) -> Result<(), GameInputError> {
        let (client, locked_pointer, relative_pointer, surface, position) = {
            let mut state = self.state.borrow_mut();
            let position = point(
                px(state.bounds.size.width.0 * 0.5),
                px(state.bounds.size.height.0 * 0.5),
            );
            (
                state.client.clone(),
                state.locked_pointer.take(),
                state.relative_pointer.take(),
                state.surface.clone(),
                position,
            )
        };

        if let Some(locked_pointer) = locked_pointer {
            locked_pointer
                .set_cursor_position_hint(f64::from(position.x.0), f64::from(position.y.0));
            surface.commit();
            locked_pointer.destroy();
        }
        if let Some(relative_pointer) = relative_pointer {
            relative_pointer.destroy();
        }
        client.pointer_lock_released(self);
        self.state.borrow_mut().pointer_lock.unlock();
        Ok(())
    }

    pub(crate) fn compositor_pointer_locked(
        &self,
        locked_pointer: &zwp_locked_pointer_v1::ZwpLockedPointerV1,
    ) {
        let client = {
            let mut state = self.state.borrow_mut();
            if !state
                .locked_pointer
                .as_ref()
                .is_some_and(|owned| owned.id() == locked_pointer.id())
            {
                return;
            }
            state.pointer_lock.lock();
            state.client.clone()
        };
        client.pointer_lock_became_active(self);
    }

    pub(crate) fn compositor_pointer_unlocked(
        &self,
        locked_pointer: &zwp_locked_pointer_v1::ZwpLockedPointerV1,
    ) {
        if !self
            .state
            .borrow()
            .locked_pointer
            .as_ref()
            .is_some_and(|owned| owned.id() == locked_pointer.id())
        {
            return;
        }
        self.release_native_pointer_lock().ok();
    }

    pub(crate) fn accepts_relative_pointer(
        &self,
        relative_pointer: &zwp_relative_pointer_v1::ZwpRelativePointerV1,
    ) -> bool {
        let state = self.state.borrow();
        state.pointer_lock.status() == PointerLockStatus::Locked
            && state
                .relative_pointer
                .as_ref()
                .is_some_and(|owned| owned.id() == relative_pointer.id())
    }

    pub(crate) fn pointer_lock_anchor(&self) -> Point<Pixels> {
        let state = self.state.borrow();
        point(
            px(state.bounds.size.width.0 * 0.5),
            px(state.bounds.size.height.0 * 0.5),
        )
    }

    pub fn app_id(&self) -> Option<String> {
        self.state.borrow().app_id.clone()
    }

    pub fn frame(&self) {
        let mut state = self.state.borrow_mut();
        state.frame_callback_requested = false;
        let forced = state.force_next_frame;
        if !forced && !state.frame_callback_active {
            return;
        }
        if forced {
            state.force_next_frame = false;
        }
        state.surface.frame(&state.globals.qh, state.surface.id());
        state.frame_callback_requested = true;
        state.resize_throttle = false;
        drop(state);

        let mut callback = self.callbacks.borrow_mut().request_frame.take();
        if let Some(ref mut fun) = callback {
            let options = RequestFrameOptions {
                force_render: forced,
                ..Default::default()
            };
            super::super::catch_platform_callback("frame request", (), || fun(options));
        }
        self.callbacks.borrow_mut().request_frame = callback;
    }

    pub fn handle_xdg_surface_event(&self, event: xdg_surface::Event) {
        if let xdg_surface::Event::Configure { serial } = event {
            {
                let mut state = self.state.borrow_mut();
                if let Some(window_controls) = state.in_progress_window_controls.take() {
                    state.window_controls = window_controls;

                    drop(state);
                    let mut callback = self.callbacks.borrow_mut().appearance_changed.take();
                    if let Some(ref mut appearance_changed) = callback {
                        super::super::catch_platform_callback("appearance change", (), || {
                            appearance_changed()
                        });
                    }
                    self.callbacks.borrow_mut().appearance_changed = callback;
                }
            }
            {
                let mut state = self.state.borrow_mut();

                if let Some(mut configure) = state.in_progress_configure.take() {
                    let got_unmaximized = state.maximized && !configure.maximized;
                    state.fullscreen = configure.fullscreen;
                    state.maximized = configure.maximized;
                    state.tiling = configure.tiling;
                    // Limit interactive resizes to once per vblank
                    if configure.resizing && state.resize_throttle {
                        return;
                    } else if configure.resizing {
                        state.resize_throttle = true;
                    }
                    if !configure.fullscreen && !configure.maximized {
                        configure.size = if got_unmaximized {
                            Some(state.window_bounds.size)
                        } else {
                            compute_outer_size(state.inset(), configure.size, state.tiling)
                        };
                        if let Some(size) = configure.size {
                            state.window_bounds = Bounds {
                                origin: Point::default(),
                                size,
                            };
                        }
                    }
                    drop(state);
                    if let Some(size) = configure.size {
                        self.resize(size);
                    }
                }
            }
            let mut state = self.state.borrow_mut();
            let Some(xdg_surface) = state.xdg_surface.clone() else {
                return;
            };
            xdg_surface.ack_configure(serial);

            let window_geometry = inset_by_tiling(
                state.bounds.map_origin(|_| px(0.0)),
                state.inset(),
                state.tiling,
            )
            .map(|v| v.0 as i32)
            .map_size(|v| if v <= 0 { 1 } else { v });

            xdg_surface.set_window_geometry(
                window_geometry.origin.x,
                window_geometry.origin.y,
                window_geometry.size.width,
                window_geometry.size.height,
            );

            // A window created with `show: false` defers its first real paint until
            // `.show()` is called, which forces one through the same gate bypass.
            let should_paint = !state.acknowledged_first_configure && state.visible;
            state.acknowledged_first_configure = true;
            drop(state);
            if should_paint {
                self.frame();
            }
        }
    }

    /// Handles configure/closed events for [`WindowKind::Overlay`] windows backed by a
    /// wlr-layer-shell surface. The layer-shell configure cycle is simpler than xdg's:
    /// the compositor proposes a size, we acknowledge the serial, resize to the proposed
    /// (or our requested) size, and drive the first frame. Returns `true` when the
    /// compositor has closed the surface and the window should be torn down.
    pub fn handle_layer_surface_event(&self, event: zwlr_layer_surface_v1::Event) -> bool {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                let mut state = self.state.borrow_mut();
                let Some(layer_surface) = state.layer_surface.clone() else {
                    return false;
                };
                layer_surface.ack_configure(serial);

                // A zero dimension means "pick your own size"; keep the requested bounds.
                let new_size = if width > 0 && height > 0 {
                    Some(size(px(width as f32), px(height as f32)))
                } else {
                    None
                };
                if let Some(new_size) = new_size {
                    state.window_bounds = Bounds {
                        origin: Point::default(),
                        size: new_size,
                    };
                }

                // A window created with `show: false` defers its first real paint until
                // `.show()` is called, which forces one through the same gate bypass.
                let should_paint = !state.acknowledged_first_configure && state.visible;
                state.acknowledged_first_configure = true;
                if should_paint {
                    state.force_next_frame = true;
                }
                drop(state);
                if let Some(new_size) = new_size {
                    self.resize(new_size);
                }
                if should_paint {
                    self.frame();
                }
                false
            }
            zwlr_layer_surface_v1::Event::Closed => true,
            _ => false,
        }
    }

    pub fn handle_toplevel_decoration_event(&self, event: zxdg_toplevel_decoration_v1::Event) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            match mode {
                WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ServerSide) => {
                    self.state.borrow_mut().decorations = WindowDecorations::Server;
                    self.notify_appearance_changed();
                }
                WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ClientSide) => {
                    self.state.borrow_mut().decorations = WindowDecorations::Client;
                    // Update background to be transparent
                    self.notify_appearance_changed();
                }
                WEnum::Value(_) => {
                    log::warn!("Unknown decoration mode");
                }
                WEnum::Unknown(v) => {
                    log::warn!("Unknown decoration mode: {}", v);
                }
            }
        }
    }

    pub fn handle_fractional_scale_event(&self, event: wp_fractional_scale_v1::Event) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            self.rescale(scale as f32 / 120.0);
        }
    }

    pub fn handle_toplevel_event(&self, event: xdg_toplevel::Event) -> bool {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let mut size = if width == 0 || height == 0 {
                    None
                } else {
                    Some(size(px(width as f32), px(height as f32)))
                };

                let states = extract_states::<xdg_toplevel::State>(&states);

                let mut tiling = Tiling::default();
                let mut fullscreen = false;
                let mut maximized = false;
                let mut resizing = false;

                for state in states {
                    match state {
                        xdg_toplevel::State::Maximized => {
                            maximized = true;
                        }
                        xdg_toplevel::State::Fullscreen => {
                            fullscreen = true;
                        }
                        xdg_toplevel::State::Resizing => resizing = true,
                        xdg_toplevel::State::TiledTop => {
                            tiling.top = true;
                        }
                        xdg_toplevel::State::TiledLeft => {
                            tiling.left = true;
                        }
                        xdg_toplevel::State::TiledRight => {
                            tiling.right = true;
                        }
                        xdg_toplevel::State::TiledBottom => {
                            tiling.bottom = true;
                        }
                        _ => {
                            // noop
                        }
                    }
                }

                if fullscreen || maximized {
                    tiling = Tiling::tiled();
                }

                let mut state = self.state.borrow_mut();
                state.in_progress_configure = Some(InProgressConfigure {
                    size,
                    fullscreen,
                    maximized,
                    resizing,
                    tiling,
                });

                false
            }
            xdg_toplevel::Event::Close => {
                let mut cb = self.callbacks.borrow_mut();
                if let Some(mut should_close) = cb.should_close.take() {
                    drop(cb);
                    let result = super::super::catch_platform_callback(
                        "window should-close",
                        false,
                        &mut should_close,
                    );
                    self.callbacks.borrow_mut().should_close = Some(should_close);
                    if result {
                        self.close();
                    }
                    result
                } else {
                    true
                }
            }
            xdg_toplevel::Event::WmCapabilities { capabilities } => {
                let mut window_controls = WindowControls::default();

                let states = extract_states::<xdg_toplevel::WmCapabilities>(&capabilities);

                for state in states {
                    match state {
                        xdg_toplevel::WmCapabilities::Maximize => {
                            window_controls.maximize = true;
                        }
                        xdg_toplevel::WmCapabilities::Minimize => {
                            window_controls.minimize = true;
                        }
                        xdg_toplevel::WmCapabilities::Fullscreen => {
                            window_controls.fullscreen = true;
                        }
                        xdg_toplevel::WmCapabilities::WindowMenu => {
                            window_controls.window_menu = true;
                        }
                        _ => {}
                    }
                }

                let mut state = self.state.borrow_mut();
                state.in_progress_window_controls = Some(window_controls);
                false
            }
            _ => false,
        }
    }

    #[allow(clippy::mutable_key_type)]
    pub fn handle_surface_event(
        &self,
        event: wl_surface::Event,
        outputs: HashMap<ObjectId, Output>,
    ) {
        let mut state = self.state.borrow_mut();

        match event {
            wl_surface::Event::Enter { output } => {
                let id = output.id();

                let Some(output) = outputs.get(&id) else {
                    return;
                };

                state.outputs.insert(id, output.clone());

                let scale = state.primary_output_scale();

                // We use `PreferredBufferScale` instead to set the scale if it's available
                if state.surface.version() < wl_surface::EVT_PREFERRED_BUFFER_SCALE_SINCE {
                    state.surface.set_buffer_scale(scale);
                    drop(state);
                    self.rescale(scale as f32);
                }
            }
            wl_surface::Event::Leave { output } => {
                state.outputs.remove(&output.id());

                let scale = state.primary_output_scale();

                // We use `PreferredBufferScale` instead to set the scale if it's available
                if state.surface.version() < wl_surface::EVT_PREFERRED_BUFFER_SCALE_SINCE {
                    state.surface.set_buffer_scale(scale);
                    drop(state);
                    self.rescale(scale as f32);
                }
            }
            wl_surface::Event::PreferredBufferScale { factor } => {
                // We use `WpFractionalScale` instead to set the scale if it's available
                if state.globals.fractional_scale_manager.is_none() {
                    state.surface.set_buffer_scale(factor);
                    drop(state);
                    self.rescale(factor as f32);
                }
            }
            _ => {}
        }
    }

    pub fn handle_ime(&self, ime: ImeInput) {
        let mut state = self.state.borrow_mut();
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            match ime {
                ImeInput::InsertText(text) => {
                    input_handler.replace_text_in_range(None, &text);
                }
                ImeInput::SetMarkedText(text) => {
                    input_handler.replace_and_mark_text_in_range(None, &text, None);
                }
                ImeInput::UnmarkText => {
                    input_handler.unmark_text();
                }
                ImeInput::DeleteText => {
                    if let Some(marked) = input_handler.marked_text_range() {
                        input_handler.replace_text_in_range(Some(marked), "");
                    }
                }
            }
            self.state.borrow_mut().input_handler = Some(input_handler);
        }
    }

    pub fn get_ime_area(&self) -> Option<Bounds<Pixels>> {
        let mut state = self.state.borrow_mut();
        let mut bounds: Option<Bounds<Pixels>> = None;
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            if let Some(selection) = input_handler.marked_text_range() {
                bounds = input_handler.bounds_for_range(selection.start..selection.start);
            }
            self.state.borrow_mut().input_handler = Some(input_handler);
        }
        bounds
    }

    pub fn set_size_and_scale(&self, size: Option<Size<Pixels>>, scale: Option<f32>) {
        let (size, scale) = {
            let mut state = self.state.borrow_mut();
            if size.is_none_or(|size| size == state.bounds.size)
                && scale.is_none_or(|scale| scale == state.scale)
            {
                return;
            }
            if let Some(size) = size {
                state.bounds.size = size;
            }
            if let Some(scale) = scale {
                state.scale = scale;
            }
            let device_bounds = state.bounds.to_device_pixels(state.scale);
            state.renderer.update_drawable_size(device_bounds.size);
            (state.bounds.size, state.scale)
        };
        let mut callback = self.callbacks.borrow_mut().resize.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("window resize", (), || fun(size, scale));
        }
        self.callbacks.borrow_mut().resize = callback;

        {
            let state = self.state.borrow();
            if let Some(viewport) = &state.viewport {
                viewport.set_destination(size.width.0 as i32, size.height.0 as i32);
            }
        }
    }

    pub fn resize(&self, size: Size<Pixels>) {
        self.set_size_and_scale(Some(size), None);
    }

    pub fn rescale(&self, scale: f32) {
        self.set_size_and_scale(None, Some(scale));
    }

    pub fn close(&self) {
        let mut callbacks = self.callbacks.borrow_mut();
        if let Some(fun) = callbacks.close.take() {
            drop(callbacks);
            super::super::catch_platform_callback("window close", (), fun);
        }
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
        if let PlatformInput::KeyDown(event) = input
            && event.keystroke.modifiers.is_subset_of(&Modifiers::shift())
            && let Some(key_char) = &event.keystroke.key_char
        {
            let mut state = self.state.borrow_mut();
            if let Some(mut input_handler) = state.input_handler.take() {
                drop(state);
                input_handler.replace_text_in_range(None, key_char);
                self.state.borrow_mut().input_handler = Some(input_handler);
            }
        }
    }

    pub fn set_focused(&self, focus: bool) {
        println!("window focus {}", focus);
        self.state.borrow_mut().active = focus;

        if !focus  {
            self.release_native_pointer_lock().ok();
        }
        let mut callback = self.callbacks.borrow_mut().active_status_change.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("active status change", (), || fun(focus));
        }
        self.callbacks.borrow_mut().active_status_change = callback;
    }

    pub fn set_hovered(&self, focus: bool) {
        let mut callback = self.callbacks.borrow_mut().hover_status_change.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("hover status change", (), || fun(focus));
        }
        self.callbacks.borrow_mut().hover_status_change = callback;
    }

    pub fn set_appearance(&mut self, appearance: WindowAppearance) {
        self.state.borrow_mut().appearance = appearance;

        self.notify_appearance_changed();
    }

    fn notify_appearance_changed(&self) {
        let mut callback = self.callbacks.borrow_mut().appearance_changed.take();
        if let Some(ref mut fun) = callback {
            super::super::catch_platform_callback("appearance change", (), fun);
        }
        self.callbacks.borrow_mut().appearance_changed = callback;
    }

    pub fn primary_output_scale(&self) -> i32 {
        self.state.borrow_mut().primary_output_scale()
    }
}

fn extract_states<'a, S: TryFrom<u32> + 'a>(states: &'a [u8]) -> impl Iterator<Item = S> + 'a
where
    <S as TryFrom<u32>>::Error: 'a,
{
    states
        .chunks_exact(4)
        .flat_map(TryInto::<[u8; 4]>::try_into)
        .map(u32::from_ne_bytes)
        .flat_map(S::try_from)
}

impl rwh::HasWindowHandle for WaylandWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let surface = self.0.surface().id().as_ptr() as *mut libc::c_void;
        let c_ptr = NonNull::new(surface).ok_or(rwh::HandleError::Unavailable)?;
        let handle = rwh::WaylandWindowHandle::new(c_ptr);
        let raw_handle = rwh::RawWindowHandle::Wayland(handle);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw_handle) })
    }
}

impl rwh::HasDisplayHandle for WaylandWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let display = self
            .0
            .surface()
            .backend()
            .upgrade()
            .ok_or(rwh::HandleError::Unavailable)?
            .display_ptr() as *mut libc::c_void;

        let c_ptr = NonNull::new(display).ok_or(rwh::HandleError::Unavailable)?;
        let handle = rwh::WaylandDisplayHandle::new(c_ptr);
        let raw_handle = rwh::RawDisplayHandle::Wayland(handle);
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(raw_handle) })
    }
}

impl PlatformWindow for WaylandWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        self.borrow().maximized
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.borrow();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.window_bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.window_bounds)
        } else {
            drop(state);
            WindowBounds::Windowed(self.bounds())
        }
    }

    fn inner_window_bounds(&self) -> WindowBounds {
        let state = self.borrow();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.window_bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.window_bounds)
        } else {
            let inset = state.inset();
            drop(state);
            WindowBounds::Windowed(self.bounds().inset(inset))
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        self.borrow().bounds.size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let state = self.borrow();
        let state_ptr = self.0.clone();
        let dp_size = size.to_device_pixels(self.scale_factor());
        let width = crate::platform::safe_gpu_dimension(dp_size.width.0 as f32);
        let height = crate::platform::safe_gpu_dimension(dp_size.height.0 as f32);

        if let Some(xdg_surface) = state.xdg_surface.as_ref() {
            xdg_surface.set_window_geometry(
                state.bounds.origin.x.0 as i32,
                state.bounds.origin.y.0 as i32,
                width as i32,
                height as i32,
            );
        } else if let Some(layer_surface) = state.layer_surface.as_ref() {
            layer_surface.set_size(width, height);
        }

        state
            .globals
            .executor
            .spawn(async move { state_ptr.resize(size) })
            .detach();
    }

    fn scale_factor(&self) -> f32 {
        self.borrow().scale
    }

    fn appearance(&self) -> WindowAppearance {
        self.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        let state = self.borrow();
        state.display.as_ref().map(|(id, display)| {
            Rc::new(WaylandDisplay {
                id: id.clone(),
                name: display.name.clone(),
                bounds: display.bounds.to_pixels(state.scale),
                scale_factor: state.scale,
                refresh_mhz: display.refresh_mhz,
            }) as Rc<dyn PlatformDisplay>
        })
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.borrow()
            .client
            .get_client()
            .and_then(|client| client.borrow().mouse_location)
            .unwrap_or_default()
    }

    fn modifiers(&self) -> Modifiers {
        self.borrow()
            .client
            .get_client()
            .map(|client| client.borrow().modifiers)
            .unwrap_or_default()
    }

    fn capslock(&self) -> Capslock {
        self.borrow()
            .client
            .get_client()
            .map(|client| client.borrow().capslock)
            .unwrap_or_default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<Receiver<usize>> {
        None
    }

    fn activate(&self) {
        // Try to request an activation token. Even though the activation is likely going to be rejected,
        // KWin and Mutter can use the app_id to visually indicate we're requesting attention.
        let state = self.borrow();
        if let (Some(activation), Some(app_id)) = (&state.globals.activation, state.app_id.clone())
        {
            state.client.set_pending_activation(state.surface.id());
            let token = activation.get_activation_token(&state.globals.qh, ());
            // The serial isn't exactly important here, since the activation is probably going to be rejected anyway.
            let serial = state.client.get_serial(SerialKind::MousePress);
            token.set_app_id(app_id);
            token.set_serial(serial, &state.globals.seat);
            token.set_surface(&state.surface);
            token.commit();
        }
    }

    fn is_active(&self) -> bool {
        self.borrow().active
    }

    fn is_hovered(&self) -> bool {
        self.borrow().hovered
    }

    fn set_title(&mut self, title: &str) {
        let state = self.borrow();
        if let Some(toplevel) = state.toplevel.as_ref() {
            toplevel.set_title(title.to_string());
        }
        // Keep the tab manager's title in sync for tabbed_windows() results.
        state.tab_manager.set_title(title.to_owned().into());
    }

    fn set_app_id(&mut self, app_id: &str) {
        let mut state = self.borrow_mut();
        if let Some(toplevel) = state.toplevel.as_ref() {
            toplevel.set_app_id(app_id.to_owned());
        }
        state.app_id = Some(app_id.to_owned());
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        let mut state = self.borrow_mut();
        state.background_appearance = background_appearance;
        update_window(state);
    }

    fn set_opacity(&self, _opacity: f32) {}

    fn set_always_on_top(&self, _always_on_top: bool) {}

    fn set_frame_polling(&self, active: bool) {
        let should_kick = {
            let mut state = self.borrow_mut();
            if state.frame_callback_active == active {
                return;
            }

            state.frame_callback_active = active;
            active && !state.frame_callback_requested
        };

        if should_kick {
            self.0.frame();
        }
    }

    fn minimize(&self) {
        if let Some(toplevel) = self.borrow().toplevel.as_ref() {
            toplevel.set_minimized();
        }
    }

    fn zoom(&self) {
        let state = self.borrow();
        let Some(toplevel) = state.toplevel.as_ref() else {
            return;
        };
        if !state.maximized {
            toplevel.set_maximized();
        } else {
            toplevel.unset_maximized();
        }
    }

    fn toggle_fullscreen(&self) {
        let mut state = self.borrow_mut();
        let Some(toplevel) = state.toplevel.clone() else {
            return;
        };
        if !state.fullscreen {
            toplevel.set_fullscreen(None);
        } else {
            toplevel.unset_fullscreen();
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.borrow().fullscreen
    }

    fn close(&self) {
        let callback = self.0.callbacks.borrow_mut().should_close.take();
        let should_close = if let Some(mut should_close) = callback {
            let result = super::super::catch_platform_callback(
                "window should-close",
                false,
                &mut should_close,
            );
            self.0.callbacks.borrow_mut().should_close = Some(should_close);
            result
        } else {
            true
        };
        if should_close {
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
        let pointer_lock = if self.0.state.borrow().client.pointer_lock_available() {
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
        self.0.callbacks.borrow_mut().hover_status_change = Some(callback);
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
        linux_webview::sync_wayland_webviews(&self.0, webviews);
    }

    #[cfg(any())]
    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        linux_webview::dispatch_wayland_webview_command(&self.0, command)
    }

    fn print(&mut self, job: crate::PlatformPrintJob) -> anyhow::Result<()> {
        crate::platform::linux::print::print_silent(job)
    }

    fn show_print_dialog(&mut self, job: crate::PlatformPrintJob) -> anyhow::Result<()> {
        let surface = self.borrow().surface.clone();
        smol::block_on(async move {
            let parent = ashpd::WindowIdentifier::from_wayland(&surface).await;
            crate::platform::linux::print::show_print_dialog(job, parent).await
        })
    }

    fn export_scene_png(
        &self,
        scene: &Scene,
    ) -> std::result::Result<crate::Image, crate::WindowCaptureError> {
        let readback = self
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
        let mut state = self.borrow_mut();
        state.renderer.draw(scene);
    }

    fn completed_frame(&self) {
        let state = self.borrow();
        state.surface.commit();
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        let state = self.borrow();
        state.renderer.sprite_atlas().clone()
    }

    fn show_window_menu(&self, position: Point<Pixels>) {
        let state = self.borrow();
        let Some(toplevel) = state.toplevel.as_ref() else {
            return;
        };
        let serial = state.client.get_serial(SerialKind::MousePress);
        toplevel.show_window_menu(
            &state.globals.seat,
            serial,
            position.x.0 as i32,
            position.y.0 as i32,
        );
    }

    fn start_window_move(&self) {
        let state = self.borrow();
        let Some(toplevel) = state.toplevel.as_ref() else {
            return;
        };
        let serial = state.client.get_serial(SerialKind::MousePress);
        toplevel._move(&state.globals.seat, serial);
    }

    fn start_window_resize(&self, edge: crate::ResizeEdge) {
        let state = self.borrow();
        let Some(toplevel) = state.toplevel.as_ref() else {
            return;
        };
        toplevel.resize(
            &state.globals.seat,
            state.client.get_serial(SerialKind::MousePress),
            edge.to_xdg(),
        )
    }

    fn window_decorations(&self) -> Decorations {
        let state = self.borrow();
        match state.decorations {
            WindowDecorations::Server => Decorations::Server,
            WindowDecorations::Client => Decorations::Client {
                tiling: state.tiling,
            },
        }
    }

    fn request_decorations(&self, decorations: WindowDecorations) {
        let mut state = self.borrow_mut();
        state.decorations = decorations;
        if let Some(decoration) = state.decoration.as_ref() {
            decoration.set_mode(decorations.to_xdg());
            update_window(state);
        }
    }

    fn window_controls(&self) -> WindowControls {
        self.borrow().window_controls
    }

    fn set_client_inset(&self, inset: Pixels) {
        let mut state = self.borrow_mut();
        if Some(inset) != state.client_inset {
            state.client_inset = Some(inset);
            update_window(state);
        }
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let state = self.borrow();
        state.client.update_ime_position(bounds);
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.borrow().renderer.gpu_specs().into()
    }

    fn show(&self) {
        let mut state = self.borrow_mut();
        let was_hidden = !state.visible;
        state.visible = true;
        // A window created with `show: false` skipped its first real paint (see
        // `handle_xdg_surface_event`/`handle_layer_surface_event`). Force one through
        // now via the same gate bypass, rather than requesting a plain frame callback:
        // that would route through the frame_callback_active-gated path and could
        // silently no-op for the same reason the layer-shell bootstrap used to.
        if was_hidden && state.acknowledged_first_configure {
            state.force_next_frame = true;
        }
        drop(state);
        if was_hidden {
            self.0.frame();
        }
    }

    fn hide(&self) {
        let mut state = self.borrow_mut();
        state.visible = false;
        state.surface.attach(None, 0, 0);
        state.surface.commit();
    }

    fn is_visible(&self) -> bool {
        self.borrow().visible
    }

    fn show_character_palette(&self) {
        super::super::character_palette::show_character_palette();
    }

    fn set_mouse_passthrough(&self, passthrough: bool) {
        let state = self.borrow();
        if passthrough {
            let region = state
                .globals
                .compositor
                .create_region(&state.globals.qh, ());
            state.surface.set_input_region(Some(&region));
            region.destroy();
        } else {
            state.surface.set_input_region(None);
        }
        state.surface.commit();
    }

    fn set_tabbing_identifier(&self, identifier: Option<String>) {
        self.borrow().tab_manager.set_tabbing_identifier(identifier);
    }

    fn merge_all_windows(&self) {
        self.borrow().tab_manager.merge_all_windows();
    }

    fn move_tab_to_new_window(&self) {
        self.borrow().tab_manager.move_tab_to_new_window();
    }

    fn tabbed_windows(&self) -> Option<Vec<crate::SystemWindowTab>> {
        self.borrow().tab_manager.tabbed_windows()
    }

    fn display_refresh_rate(&self) -> Option<f32> {
        self.borrow().display_refresh_rate()
    }

    fn update_accessibility_tree(
        &mut self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        let state = self.borrow();
        state.accessibility_root.update_tree(tree);
        state.accessibility_root.drain_actions(tree)
    }
}

fn update_window(mut state: RefMut<WaylandWindowState>) {
    let opaque = !state.is_transparent();

    state.renderer.update_transparency(!opaque);
    let mut opaque_area = state.window_bounds.map(|v| v.0 as i32);
    opaque_area.inset(state.inset().0 as i32);

    let region = state
        .globals
        .compositor
        .create_region(&state.globals.qh, ());
    region.add(
        opaque_area.origin.x,
        opaque_area.origin.y,
        opaque_area.size.width,
        opaque_area.size.height,
    );

    // Note that rounded corners make this rectangle API hard to work with.
    // As this is common when using CSD, let's just disable this API.
    if state.background_appearance == WindowBackgroundAppearance::Opaque
        && state.decorations == WindowDecorations::Server
    {
        // Promise the compositor that this region of the window surface
        // contains no transparent pixels. This allows the compositor to skip
        // updating whatever is behind the surface for better performance.
        state.surface.set_opaque_region(Some(&region));
    } else {
        state.surface.set_opaque_region(None);
    }

    if let Some(ref blur_manager) = state.globals.blur_manager {
        if state.background_appearance == WindowBackgroundAppearance::Blurred {
            if state.blur.is_none() {
                let blur = blur_manager.create(&state.surface, &state.globals.qh, ());
                state.blur = Some(blur);
            }
            if let Some(blur) = state.blur.as_ref() {
                blur.commit();
            }
        } else {
            // It probably doesn't hurt to clear the blur for opaque windows
            blur_manager.unset(&state.surface);
            if let Some(b) = state.blur.take() {
                b.release()
            }
        }
    }

    region.destroy();
}

impl WindowDecorations {
    fn to_xdg(self) -> zxdg_toplevel_decoration_v1::Mode {
        match self {
            WindowDecorations::Client => zxdg_toplevel_decoration_v1::Mode::ClientSide,
            WindowDecorations::Server => zxdg_toplevel_decoration_v1::Mode::ServerSide,
        }
    }
}

impl ResizeEdge {
    fn to_xdg(self) -> xdg_toplevel::ResizeEdge {
        match self {
            ResizeEdge::Top => xdg_toplevel::ResizeEdge::Top,
            ResizeEdge::TopRight => xdg_toplevel::ResizeEdge::TopRight,
            ResizeEdge::Right => xdg_toplevel::ResizeEdge::Right,
            ResizeEdge::BottomRight => xdg_toplevel::ResizeEdge::BottomRight,
            ResizeEdge::Bottom => xdg_toplevel::ResizeEdge::Bottom,
            ResizeEdge::BottomLeft => xdg_toplevel::ResizeEdge::BottomLeft,
            ResizeEdge::Left => xdg_toplevel::ResizeEdge::Left,
            ResizeEdge::TopLeft => xdg_toplevel::ResizeEdge::TopLeft,
        }
    }
}

/// The configuration event is in terms of the window geometry, which we are constantly
/// updating to account for the client decorations. But that's not the area we want to render
/// to, due to our intrusize CSD. So, here we calculate the 'actual' size, by adding back in the insets
fn compute_outer_size(
    inset: Pixels,
    new_size: Option<Size<Pixels>>,
    tiling: Tiling,
) -> Option<Size<Pixels>> {
    new_size.map(|mut new_size| {
        if !tiling.top {
            new_size.height += inset;
        }
        if !tiling.bottom {
            new_size.height += inset;
        }
        if !tiling.left {
            new_size.width += inset;
        }
        if !tiling.right {
            new_size.width += inset;
        }

        new_size
    })
}

fn inset_by_tiling(mut bounds: Bounds<Pixels>, inset: Pixels, tiling: Tiling) -> Bounds<Pixels> {
    if !tiling.top {
        bounds.origin.y += inset;
        bounds.size.height -= inset;
    }
    if !tiling.bottom {
        bounds.size.height -= inset;
    }
    if !tiling.left {
        bounds.origin.x += inset;
        bounds.size.width -= inset;
    }
    if !tiling.right {
        bounds.size.width -= inset;
    }

    bounds
}
