use super::accessibility::BrowserAccessibilityManager;
use super::atlas::WebAtlas;
use super::print::BrowserPrintManager;
use super::renderer::WebGlSceneRenderer;
use super::webview::BrowserWebViewManager;
use super::{WebClipboardState, WebDisplay, WebWindowRegistry};
use crate::platform::web_clipboard_limits::{
    BrowserClipboardBudget, BrowserClipboardItemKind, BrowserClipboardLimitError,
    MAX_BROWSER_CLIPBOARD_ITEMS, validate_browser_clipboard_item_count,
};
use crate::{
    AnyWindowHandle, Bounds, Capslock, ClipboardEntry, ClipboardItem, DevicePixels,
    DispatchEventResult, ExternalDropData, ExternalFile, FileDropEvent, GameInputAvailability,
    GameInputCapabilities, GameInputError, GameInputErrorKind, GamepadButtonState, GamepadMapping,
    GamepadSnapshot, GamepadState, GpuSpecs, Image as ClipboardImage,
    ImageFormat as ClipboardImageFormat, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    MouseButton, NavigationDirection, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput,
    PlatformInputHandler, PlatformPrintJob, PlatformWindow, Point, PointerButtons, PointerId,
    PointerInputEvent, PointerLockStatus, PointerPhase, PointerSample, PointerType, PromptButton,
    PromptLevel, RequestFrameOptions, Scene, ScrollDelta, ScrollWheelEvent, Size, TouchPhase,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowControls,
    WindowParams, point, px, size,
};
use anyhow::{Context as _, Result, anyhow};
use raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    WebCanvasWindowHandle, WebDisplayHandle, WindowHandle,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    ClipboardEvent, CompositionEvent, DataTransfer, Document, DragEvent, Event, EventTarget, File,
    Gamepad as BrowserGamepad, GamepadMappingType, HtmlCanvasElement, HtmlElement,
    HtmlInputElement, InputEvent, KeyboardEvent, PointerEvent as DomPointerEvent, ResizeObserver,
    WheelEvent,
};

struct WebEventListener {
    target: EventTarget,
    name: String,
    callback: Closure<dyn FnMut(Event)>,
}

#[derive(Default)]
struct WebWindowCallbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active: Option<Box<dyn FnMut(bool)>>,
    hover: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    hit_test: Option<Box<dyn FnMut() -> Option<WindowControlArea>>>,
    appearance: Option<Box<dyn FnMut()>>,
}

#[derive(Clone, Copy)]
struct BrowserWindowDrag {
    pointer_id: i32,
    start_client_x: i32,
    start_client_y: i32,
    start_left: f32,
    start_top: f32,
}

struct WebWindowInner {
    handle: AnyWindowHandle,
    canvas: HtmlCanvasElement,
    surface_host: Option<HtmlElement>,
    registry: Rc<RefCell<WebWindowRegistry>>,
    application_hidden: Rc<Cell<bool>>,
    closed: Cell<bool>,
    movable: bool,
    window_drag: Cell<Option<BrowserWindowDrag>>,
    ime_input: HtmlInputElement,
    renderer: RefCell<WebGlSceneRenderer>,
    atlas: Arc<WebAtlas>,
    display: Rc<WebDisplay>,
    bounds: Cell<Bounds<Pixels>>,
    scale_factor: Cell<f32>,
    mouse_position: Cell<Point<Pixels>>,
    modifiers: Cell<Modifiers>,
    capslock: Cell<Capslock>,
    pressed_button: Cell<Option<MouseButton>>,
    active_pointer_ids: RefCell<HashSet<i32>>,
    active: Cell<bool>,
    hovered: Cell<bool>,
    visible: Cell<bool>,
    context_lost: Cell<bool>,
    frame_polling: Cell<bool>,
    pointer_lock_status: Cell<PointerLockStatus>,
    pointer_lock_error: RefCell<Option<GameInputError>>,
    raf_handle: Cell<Option<i32>>,
    callbacks: RefCell<WebWindowCallbacks>,
    input_handler: RefCell<Option<PlatformInputHandler>>,
    clipboard: Arc<parking_lot::Mutex<WebClipboardState>>,
    pending_clipboard_write: Cell<Option<u64>>,
    pending_paste: RefCell<Option<Keystroke>>,
    ime_composing: Cell<bool>,
    ignore_next_text_input: Cell<bool>,
    event_callbacks: RefCell<Vec<WebEventListener>>,
    raf_callback: RefCell<Option<Closure<dyn FnMut(f64)>>>,
    resize_callback: RefCell<Option<Closure<dyn FnMut(js_sys::Array, ResizeObserver)>>>,
    resize_observer: RefCell<Option<ResizeObserver>>,
    accessibility: RefCell<BrowserAccessibilityManager>,
    print: RefCell<BrowserPrintManager>,
    webviews: RefCell<BrowserWebViewManager>,
}

/// A browser window backed by either the primary `#blade` canvas or an
/// independent retained secondary surface inside the page.
#[derive(Clone)]
pub(crate) struct WebWindow(Rc<WebWindowInner>);

struct BrowserWindowSurface {
    canvas: HtmlCanvasElement,
    host: Option<HtmlElement>,
    primary: bool,
    committed: bool,
}

impl BrowserWindowSurface {
    fn commit(mut self) -> (HtmlCanvasElement, Option<HtmlElement>) {
        self.committed = true;
        (self.canvas.clone(), self.host.clone())
    }
}

impl Drop for BrowserWindowSurface {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Some(host) = &self.host {
            host.remove();
        } else if self.primary {
            let _ = self.canvas.remove_attribute("data-kael-window-surface-id");
            let _ = self.canvas.remove_attribute("data-kael-window-primary");
        }
    }
}

fn create_window_surface(
    document: &Document,
    params: &WindowParams,
    primary: bool,
    surface_id: u64,
) -> Result<BrowserWindowSurface> {
    let surface_id = surface_id.to_string();
    if primary {
        let canvas = document
            .get_element_by_id("blade")
            .context("browser backend requires a <canvas id=\"blade\">")?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| anyhow!("#blade is not a canvas element"))?;
        anyhow::ensure!(
            !canvas.has_attribute("data-kael-window-surface-id"),
            "the primary #blade browser canvas is already attached"
        );
        canvas
            .set_attribute("data-kael-window-surface-id", &surface_id)
            .map_err(js_error)?;
        canvas
            .set_attribute("data-kael-window-primary", "true")
            .map_err(js_error)?;
        return Ok(BrowserWindowSurface {
            canvas,
            host: None,
            primary: true,
            committed: false,
        });
    }

    let body = document
        .body()
        .context("browser Document has no body for a secondary Kael window")?;
    let host = document
        .create_element("section")
        .map_err(js_error)?
        .dyn_into::<HtmlElement>()
        .map_err(|_| anyhow!("failed to create secondary browser window host"))?;
    host.set_id(&format!("kael-window-{surface_id}"));
    host.set_attribute("data-kael-window-host", "true")
        .map_err(js_error)?;
    host.set_attribute("data-kael-window-surface-id", &surface_id)
        .map_err(js_error)?;
    host.set_attribute("role", "presentation")
        .map_err(js_error)?;
    let style = host.style();
    for (name, value) in [
        ("position", "fixed".to_owned()),
        ("left", format!("{}px", params.bounds.origin.x.0)),
        ("top", format!("{}px", params.bounds.origin.y.0)),
        (
            "width",
            format!("{}px", params.bounds.size.width.0.max(1.0)),
        ),
        (
            "height",
            format!("{}px", params.bounds.size.height.0.max(1.0)),
        ),
        ("box-sizing", "border-box".to_owned()),
        ("overflow", "hidden".to_owned()),
        ("isolation", "isolate".to_owned()),
        ("background", "transparent".to_owned()),
    ] {
        style.set_property(name, &value).map_err(js_error)?;
    }
    if let Some(minimum) = params.window_min_size {
        style
            .set_property("min-width", &format!("{}px", minimum.width.0.max(1.0)))
            .map_err(js_error)?;
        style
            .set_property("min-height", &format!("{}px", minimum.height.0.max(1.0)))
            .map_err(js_error)?;
    }
    if params.is_resizable {
        style.set_property("resize", "both").map_err(js_error)?;
    }
    if params.mouse_passthrough {
        style
            .set_property("pointer-events", "none")
            .map_err(js_error)?;
    }

    let canvas = document
        .create_element("canvas")
        .map_err(js_error)?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| anyhow!("failed to create secondary browser window canvas"))?;
    canvas.set_id(&format!("kael-window-canvas-{surface_id}"));
    canvas
        .set_attribute("data-kael-window-surface-id", &surface_id)
        .map_err(js_error)?;
    canvas
        .set_attribute("data-kael-window-primary", "false")
        .map_err(js_error)?;
    let canvas_style = canvas.style();
    for (name, value) in [
        ("display", "block"),
        ("width", "100%"),
        ("height", "100%"),
        ("outline", "none"),
        ("touch-action", "none"),
    ] {
        canvas_style.set_property(name, value).map_err(js_error)?;
    }
    host.append_child(&canvas).map_err(js_error)?;
    body.append_child(&host).map_err(js_error)?;
    Ok(BrowserWindowSurface {
        canvas,
        host: Some(host),
        primary: false,
        committed: false,
    })
}

impl WebWindow {
    pub(crate) fn new(
        handle: AnyWindowHandle,
        params: WindowParams,
        display: Rc<WebDisplay>,
        clipboard: Arc<parking_lot::Mutex<WebClipboardState>>,
        registry: Rc<RefCell<WebWindowRegistry>>,
        application_hidden: Rc<Cell<bool>>,
        primary: bool,
        surface_id: u64,
    ) -> Result<Self> {
        let browser = web_sys::window().context("browser Window is unavailable")?;
        let document = browser
            .document()
            .context("browser Document is unavailable")?;
        let surface = create_window_surface(&document, &params, primary, surface_id)?;
        let canvas = surface.canvas.clone();
        let surface_host = surface.host.clone();
        let ime_input = create_ime_input(&document)?;
        canvas.set_tab_index(0);
        let style = canvas.style();
        style.set_property("display", "block").map_err(js_error)?;
        style.set_property("outline", "none").map_err(js_error)?;
        style
            .set_property("touch-action", "none")
            .map_err(js_error)?;

        let rect = canvas.get_bounding_client_rect();
        let mut logical_size = size(px(rect.width() as f32), px(rect.height() as f32));
        if logical_size.width.0 <= 0.0 || logical_size.height.0 <= 0.0 {
            logical_size = params.bounds.size;
            style
                .set_property("width", &format!("{}px", logical_size.width.0.max(1.0)))
                .map_err(js_error)?;
            style
                .set_property("height", &format!("{}px", logical_size.height.0.max(1.0)))
                .map_err(js_error)?;
        }
        let scale_factor = browser.device_pixel_ratio().max(0.1) as f32;
        let device_size = device_size(logical_size, scale_factor);
        canvas.set_width(device_size.width.0 as u32);
        canvas.set_height(device_size.height.0 as u32);

        let renderer = WebGlSceneRenderer::new(&canvas, device_size)?;
        let atlas = renderer.atlas();
        canvas
            .set_attribute("data-kael-webgl-context", "active")
            .map_err(js_error)?;
        let accessibility = BrowserAccessibilityManager::new(&canvas)?;
        let print = BrowserPrintManager::new(&canvas)?;
        let webviews = BrowserWebViewManager::new(&canvas)?;
        let pointer_lock_status = if browser_pointer_lock_supported(&canvas, &document) {
            PointerLockStatus::Unlocked
        } else {
            PointerLockStatus::Unsupported
        };
        let window = Self(Rc::new(WebWindowInner {
            handle,
            canvas,
            surface_host,
            registry: registry.clone(),
            application_hidden,
            closed: Cell::new(false),
            movable: params.is_movable,
            window_drag: Cell::new(None),
            ime_input,
            renderer: RefCell::new(renderer),
            atlas,
            display,
            bounds: Cell::new(Bounds::new(params.bounds.origin, logical_size)),
            scale_factor: Cell::new(scale_factor),
            mouse_position: Cell::new(Point::default()),
            modifiers: Cell::new(Modifiers::default()),
            capslock: Cell::new(Capslock::default()),
            pressed_button: Cell::new(None),
            active_pointer_ids: RefCell::new(HashSet::new()),
            active: Cell::new(params.focus && document.has_focus().unwrap_or(true)),
            hovered: Cell::new(false),
            visible: Cell::new(params.show),
            context_lost: Cell::new(false),
            frame_polling: Cell::new(false),
            pointer_lock_status: Cell::new(pointer_lock_status),
            pointer_lock_error: RefCell::new(None),
            raf_handle: Cell::new(None),
            callbacks: RefCell::new(WebWindowCallbacks::default()),
            input_handler: RefCell::new(None),
            clipboard,
            pending_clipboard_write: Cell::new(None),
            pending_paste: RefCell::new(None),
            ime_composing: Cell::new(false),
            ignore_next_text_input: Cell::new(false),
            event_callbacks: RefCell::new(Vec::new()),
            raf_callback: RefCell::new(None),
            resize_callback: RefCell::new(None),
            resize_observer: RefCell::new(None),
            accessibility: RefCell::new(accessibility),
            print: RefCell::new(print),
            webviews: RefCell::new(webviews),
        }));
        let _ = surface.commit();
        let weak = Rc::downgrade(&window.0);
        window.0.accessibility.borrow_mut().set_wake(move || {
            let Some(inner) = weak.upgrade() else { return };
            let mut callback = inner.callbacks.borrow_mut().request_frame.take();
            if let Some(callback_fn) = callback.as_mut() {
                callback_fn(RequestFrameOptions {
                    require_presentation: false,
                    force_render: true,
                });
            }
            inner.callbacks.borrow_mut().request_frame = callback;
            schedule_frame_inner(&inner);
        });
        window.install_animation_frame()?;
        window.install_resize_observer()?;
        window.install_webgl_context_events()?;
        window.install_input_events()?;
        window.install_game_input_events()?;
        window.install_clipboard_events()?;
        window.install_file_drop_events()?;
        window.install_webview_events()?;
        let weak = Rc::downgrade(&window.0);
        let sync_visibility = Rc::new(move || {
            if let Some(inner) = weak.upgrade() {
                sync_window_visibility(&inner);
            }
        });
        registry.borrow_mut().register(
            handle,
            window.0.canvas.clone(),
            window.0.surface_host.clone(),
            sync_visibility,
            params.focus,
        );
        if params.focus {
            let _ = window.0.canvas.focus();
        }
        sync_window_visibility(&window.0);
        Ok(window)
    }

    fn install_animation_frame(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let callback = Closure::wrap(Box::new(move |timestamp: f64| {
            let Some(inner) = weak.upgrade() else { return };
            inner.raf_handle.set(None);
            if !inner.frame_polling.get()
                || !inner.visible.get()
                || inner.application_hidden.get()
                || inner.context_lost.get()
                || !browser_document_is_visible()
            {
                return;
            }
            inner.display.record_animation_frame(timestamp);
            let mut callback = inner.callbacks.borrow_mut().request_frame.take();
            if let Some(callback_fn) = callback.as_mut() {
                callback_fn(RequestFrameOptions::default());
            }
            inner.callbacks.borrow_mut().request_frame = callback;
        }) as Box<dyn FnMut(f64)>);
        *self.0.raf_callback.borrow_mut() = Some(callback);
        Ok(())
    }

    fn install_resize_observer(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let callback = Closure::wrap(Box::new(
            move |_entries: js_sys::Array, _observer: ResizeObserver| {
                if let Some(inner) = weak.upgrade() {
                    resize_inner(&inner);
                }
            },
        ) as Box<dyn FnMut(js_sys::Array, ResizeObserver)>);
        let observer = ResizeObserver::new(callback.as_ref().unchecked_ref()).map_err(js_error)?;
        observer.observe(&self.0.canvas);
        *self.0.resize_callback.borrow_mut() = Some(callback);
        *self.0.resize_observer.borrow_mut() = Some(observer);
        Ok(())
    }

    fn install_webgl_context_events(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        self.add_event("webglcontextlost", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            // Preventing the default tells the browser that Kael intends to
            // restore the context rather than accepting permanent loss.
            event.prevent_default();
            inner.context_lost.set(true);
            cancel_scheduled_frame(&inner);
            let _ = inner
                .canvas
                .set_attribute("data-kael-webgl-context", "lost");
            log::warn!("browser WebGL2 context was lost; waiting for restoration");
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("webglcontextrestored", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            let size = size(
                DevicePixels(i32::try_from(inner.canvas.width()).unwrap_or(i32::MAX)),
                DevicePixels(i32::try_from(inner.canvas.height()).unwrap_or(i32::MAX)),
            );
            let (frame_count, recovery_reference) = {
                let mut renderer = inner.renderer.borrow_mut();
                (renderer.frame_count(), renderer.take_verification_pixels())
            };
            let restored = WebGlSceneRenderer::restored(
                &inner.canvas,
                size,
                inner.atlas.clone(),
                frame_count,
                recovery_reference,
            );
            match restored {
                Ok(renderer) => {
                    // The browser already discarded every object owned by the
                    // lost context generation. Deleting those stale handles
                    // after restoration raises INVALID_OPERATION and poisons
                    // the restored context's error queue. Replace the wrapper
                    // directly; normal platform shutdown still calls destroy.
                    *inner.renderer.borrow_mut() = renderer;
                    inner.context_lost.set(false);
                    let _ = inner
                        .canvas
                        .set_attribute("data-kael-webgl-context", "restored");
                    for attribute in [
                        "data-kael-pixel-readback",
                        "data-kael-pixel-changed",
                        "data-kael-pixel-luma-range",
                        "data-kael-pixel-hash",
                        "data-kael-context-differing-bytes",
                        "data-kael-context-reference-hash",
                    ] {
                        let _ = inner.canvas.remove_attribute(attribute);
                    }

                    let mut callback = inner.callbacks.borrow_mut().request_frame.take();
                    if let Some(callback_fn) = callback.as_mut() {
                        callback_fn(RequestFrameOptions {
                            require_presentation: true,
                            force_render: true,
                        });
                    }
                    inner.callbacks.borrow_mut().request_frame = callback;
                    schedule_frame_inner(&inner);
                    log::info!("browser WebGL2 context restored");
                }
                Err(error) => {
                    let _ = inner
                        .canvas
                        .set_attribute("data-kael-webgl-context", "restore-failed");
                    log::error!("failed to restore browser WebGL2 context: {error:#}");
                }
            }
        })?;
        Ok(())
    }

    fn add_event(&self, name: &str, callback: impl FnMut(Event) + 'static) -> Result<()> {
        let target: EventTarget = self.0.canvas.clone().unchecked_into();
        self.add_event_to(target, name, callback)
    }

    fn add_window_event(&self, name: &str, callback: impl FnMut(Event) + 'static) -> Result<()> {
        let target: EventTarget = web_sys::window()
            .context("browser Window is unavailable")?
            .unchecked_into();
        self.add_event_to(target, name, callback)
    }

    fn add_event_to(
        &self,
        target: EventTarget,
        name: &str,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<()> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        target
            .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
            .map_err(js_error)?;
        self.0.event_callbacks.borrow_mut().push(WebEventListener {
            target,
            name: name.to_owned(),
            callback,
        });
        Ok(())
    }

    fn install_input_events(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        self.add_event("pointerdown", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DomPointerEvent>() else {
                return;
            };
            let pointer = browser_pointer_event(&inner.canvas, &event, PointerPhase::Down, false);
            inner.mouse_position.set(pointer.position);
            inner.modifiers.set(pointer.modifiers);
            if pointer.is_primary {
                inner
                    .pressed_button
                    .set(pointer.buttons.primary_legacy_button());
            }
            inner
                .active_pointer_ids
                .borrow_mut()
                .insert(event.pointer_id());
            set_active_inner(&inner, true);
            let _ = inner.canvas.focus();
            let _ = inner.canvas.set_pointer_capture(event.pointer_id());
            if handle_window_control_pointer_down(&inner, &event) {
                event.prevent_default();
                return;
            }
            if should_prevent_default(dispatch_input(&inner, PlatformInput::Pointer(pointer))) {
                event.prevent_default();
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("pointerup", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DomPointerEvent>() else {
                return;
            };
            let pointer = browser_pointer_event(&inner.canvas, &event, PointerPhase::Up, false);
            inner.mouse_position.set(pointer.position);
            inner.modifiers.set(pointer.modifiers);
            if pointer.is_primary {
                inner
                    .pressed_button
                    .set(pointer.buttons.primary_legacy_button());
            }
            inner
                .active_pointer_ids
                .borrow_mut()
                .remove(&event.pointer_id());
            let _ = inner.canvas.release_pointer_capture(event.pointer_id());
            if inner
                .window_drag
                .get()
                .is_some_and(|drag| drag.pointer_id == event.pointer_id())
            {
                inner.window_drag.set(None);
                event.prevent_default();
                return;
            }
            if should_prevent_default(dispatch_input(&inner, PlatformInput::Pointer(pointer))) {
                event.prevent_default();
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("pointermove", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DomPointerEvent>() else {
                return;
            };
            let pointer = browser_pointer_event(&inner.canvas, &event, PointerPhase::Move, true);
            inner.mouse_position.set(pointer.position);
            inner.modifiers.set(pointer.modifiers);
            if pointer.is_primary {
                inner
                    .pressed_button
                    .set(pointer.buttons.primary_legacy_button());
            }
            if move_browser_window_drag(&inner, &event) {
                event.prevent_default();
                return;
            }
            if should_prevent_default(dispatch_input(&inner, PlatformInput::Pointer(pointer))) {
                event.prevent_default();
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("pointercancel", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DomPointerEvent>() else {
                return;
            };
            let pointer = browser_pointer_event(&inner.canvas, &event, PointerPhase::Cancel, false);
            inner.mouse_position.set(pointer.position);
            inner.modifiers.set(pointer.modifiers);
            if pointer.is_primary {
                inner.pressed_button.set(None);
            }
            inner
                .active_pointer_ids
                .borrow_mut()
                .remove(&event.pointer_id());
            let _ = inner.canvas.release_pointer_capture(event.pointer_id());
            if inner
                .window_drag
                .get()
                .is_some_and(|drag| drag.pointer_id == event.pointer_id())
            {
                inner.window_drag.set(None);
                event.prevent_default();
                return;
            }
            if should_prevent_default(dispatch_input(&inner, PlatformInput::Pointer(pointer))) {
                event.prevent_default();
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("lostpointercapture", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DomPointerEvent>() else {
                return;
            };
            if !inner
                .active_pointer_ids
                .borrow_mut()
                .remove(&event.pointer_id())
            {
                return;
            }
            let pointer = browser_pointer_event(&inner.canvas, &event, PointerPhase::Cancel, false);
            inner.mouse_position.set(pointer.position);
            inner.modifiers.set(pointer.modifiers);
            if pointer.is_primary {
                inner.pressed_button.set(None);
            }
            if inner
                .window_drag
                .get()
                .is_some_and(|drag| drag.pointer_id == event.pointer_id())
            {
                inner.window_drag.set(None);
            }
            if should_prevent_default(dispatch_input(&inner, PlatformInput::Pointer(pointer))) {
                event.prevent_default();
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("pointerenter", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            if let Ok(event) = event.dyn_into::<DomPointerEvent>() {
                let pointer =
                    browser_pointer_event(&inner.canvas, &event, PointerPhase::Enter, false);
                inner.mouse_position.set(pointer.position);
                inner.modifiers.set(pointer.modifiers);
                dispatch_input(&inner, PlatformInput::Pointer(pointer));
            }
            inner.hovered.set(true);
            invoke_bool_callback(&inner, true, false);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event("pointerleave", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            if let Ok(event) = event.dyn_into::<DomPointerEvent>() {
                let pointer =
                    browser_pointer_event(&inner.canvas, &event, PointerPhase::Leave, false);
                inner.mouse_position.set(pointer.position);
                inner.modifiers.set(pointer.modifiers);
                dispatch_input(&inner, PlatformInput::Pointer(pointer));
            }
            inner.hovered.set(false);
            invoke_bool_callback(&inner, false, false);
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("focus", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            set_active_inner(&inner, true);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event("blur", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            inner.pressed_button.set(None);
            // Focusing the hidden IME element transfers DOM focus away from
            // the canvas without deactivating the Kael window.
            if inner.input_handler.borrow().is_none() {
                set_active_inner(&inner, false);
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("keydown", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            handle_keydown_event(&inner, event);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event("keyup", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            handle_keyup_event(&inner, event);
        })?;

        let ime_target: EventTarget = self.0.ime_input.clone().unchecked_into();
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "focus", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            set_active_inner(&inner, true);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "blur", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            inner.ime_composing.set(false);
            if !canvas_has_dom_focus(&inner) {
                set_active_inner(&inner, false);
            }
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "keydown", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            handle_keydown_event(&inner, event);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "keyup", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            handle_keyup_event(&inner, event);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "compositionstart", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            if event.dyn_ref::<CompositionEvent>().is_none() {
                return;
            }
            inner.ime_composing.set(true);
            inner.ignore_next_text_input.set(false);
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "compositionupdate", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<CompositionEvent>() else {
                return;
            };
            let text = event.data().unwrap_or_default();
            let caret = text.encode_utf16().count();
            if let Some(handler) = inner.input_handler.borrow_mut().as_mut() {
                handler.replace_and_mark_text_in_range(None, &text, Some(caret..caret));
            }
            inner.ime_input.set_value("");
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target.clone(), "compositionend", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<CompositionEvent>() else {
                return;
            };
            let text = event.data().unwrap_or_default();
            if let Some(handler) = inner.input_handler.borrow_mut().as_mut() {
                handler.replace_text_in_range(None, &text);
            }
            inner.ime_composing.set(false);
            inner.ignore_next_text_input.set(true);
            inner.ime_input.set_value("");
        })?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(ime_target, "input", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<InputEvent>() else {
                return;
            };
            if inner.ime_composing.get() {
                inner.ime_input.set_value("");
                return;
            }
            if inner.ignore_next_text_input.replace(false) {
                inner.ime_input.set_value("");
                return;
            }
            let text = match event.input_type().as_str() {
                "insertLineBreak" | "insertParagraph" => Some("\n".to_owned()),
                input_type if input_type.starts_with("insert") => event.data(),
                _ => None,
            };
            if let Some(text) = text.filter(|text| !text.is_empty())
                && let Some(handler) = inner.input_handler.borrow_mut().as_mut()
            {
                handler.replace_text_in_range(None, &text);
            }
            inner.ime_input.set_value("");
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("wheel", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<WheelEvent>() else {
                return;
            };
            let rect = inner.canvas.get_bounding_client_rect();
            let position = point(
                px(event.client_x() as f32 - rect.left() as f32),
                px(event.client_y() as f32 - rect.top() as f32),
            );
            let modifiers = wheel_modifiers(&event);
            inner.mouse_position.set(position);
            inner.modifiers.set(modifiers);
            let delta = if event.delta_mode() == WheelEvent::DOM_DELTA_PIXEL {
                ScrollDelta::Pixels(point(
                    px(-event.delta_x() as f32),
                    px(-event.delta_y() as f32),
                ))
            } else {
                ScrollDelta::Lines(point(-event.delta_x() as f32, -event.delta_y() as f32))
            };
            if should_prevent_default(dispatch_input(
                &inner,
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position,
                    delta,
                    modifiers,
                    touch_phase: TouchPhase::Moved,
                    is_momentum: false,
                }),
            )) {
                event.prevent_default();
            }
        })?;
        self.add_event("contextmenu", |event| event.prevent_default())?;
        let weak = Rc::downgrade(&self.0);
        self.add_event("kael-fonts-loaded", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            inner.atlas.clear();

            let logical = inner.bounds.get().size;
            let scale = inner.scale_factor.get();
            let mut resize = inner.callbacks.borrow_mut().resize.take();
            if let Some(callback) = resize.as_mut() {
                callback(logical, scale);
            }
            inner.callbacks.borrow_mut().resize = resize;

            let mut request_frame = inner.callbacks.borrow_mut().request_frame.take();
            if let Some(callback) = request_frame.as_mut() {
                callback(RequestFrameOptions {
                    require_presentation: true,
                    force_render: true,
                });
            }
            inner.callbacks.borrow_mut().request_frame = request_frame;
        })?;

        // ResizeObserver covers CSS-size changes, while the Window resize event
        // also catches device-pixel-ratio-only changes caused by browser zoom or
        // moving the page between displays.
        let weak = Rc::downgrade(&self.0);
        self.add_window_event("resize", move |_event| {
            let Some(inner) = weak.upgrade() else { return };
            resize_inner(&inner);
        })?;

        // Browsers throttle background rAF callbacks rather than suspending an application's
        // polling intent. Cancel the pending callback while hidden and re-arm it on visibility
        // restoration, preserving animations without a background wake-up loop.
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser Document is unavailable")?;
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(
            document.unchecked_into(),
            "visibilitychange",
            move |_event| {
                let Some(inner) = weak.upgrade() else { return };
                if browser_document_is_visible() {
                    schedule_frame_inner(&inner);
                } else {
                    cancel_scheduled_frame(&inner);
                }
            },
        )?;
        Ok(())
    }

    fn install_game_input_events(&self) -> Result<()> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser Document is unavailable")?;
        let target: EventTarget = document.clone().unchecked_into();
        let change_document = document.clone();
        let weak = Rc::downgrade(&self.0);
        self.add_event_to(target.clone(), "pointerlockchange", move |_event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let locked = change_document
                .pointer_lock_element()
                .is_some_and(|element| element == inner.canvas.clone().unchecked_into());
            let previous = inner.pointer_lock_status.get();
            let next = if locked {
                PointerLockStatus::Locked
            } else if previous == PointerLockStatus::Unsupported {
                PointerLockStatus::Unsupported
            } else {
                PointerLockStatus::Unlocked
            };
            inner.pointer_lock_status.set(next);
            if next == PointerLockStatus::Locked {
                inner.pointer_lock_error.borrow_mut().take();
            }
            schedule_frame_inner(&inner);
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event_to(target, "pointerlockerror", move |_event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.pointer_lock_status.get() != PointerLockStatus::Requesting {
                return;
            }
            inner.pointer_lock_status.set(PointerLockStatus::Unlocked);
            *inner.pointer_lock_error.borrow_mut() = Some(GameInputError::new(
                GameInputErrorKind::Rejected,
                "browser rejected pointer lock; trusted activation, document policy, focus, or lock ownership may have prevented it",
            ));
            schedule_frame_inner(&inner);
        })?;
        Ok(())
    }

    fn install_clipboard_events(&self) -> Result<()> {
        let canvas_target: EventTarget = self.0.canvas.clone().unchecked_into();
        let ime_target: EventTarget = self.0.ime_input.clone().unchecked_into();
        for target in [canvas_target, ime_target] {
            let weak = Rc::downgrade(&self.0);
            self.add_event_to(target.clone(), "copy", move |event| {
                let Some(inner) = weak.upgrade() else { return };
                handle_clipboard_write_event(&inner, event);
            })?;
            let weak = Rc::downgrade(&self.0);
            self.add_event_to(target.clone(), "cut", move |event| {
                let Some(inner) = weak.upgrade() else { return };
                handle_clipboard_write_event(&inner, event);
            })?;
            let weak = Rc::downgrade(&self.0);
            self.add_event_to(target, "paste", move |event| {
                let Some(inner) = weak.upgrade() else { return };
                handle_clipboard_paste_event(&inner, event);
            })?;
        }
        Ok(())
    }

    fn install_file_drop_events(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        self.add_event("dragenter", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DragEvent>() else {
                return;
            };
            let Some(data) = event.data_transfer() else {
                return;
            };
            event.prevent_default();
            let position = drag_position(&inner.canvas, &event);
            let payload = browser_drop_payload(&data, browser_file_metadata(&data));
            if !payload.is_empty() {
                let _ = dispatch_input(
                    &inner,
                    PlatformInput::FileDrop(FileDropEvent::DataEntered {
                        position,
                        data: payload,
                    }),
                );
            }
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("dragover", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DragEvent>() else {
                return;
            };
            event.prevent_default();
            let _ = dispatch_input(
                &inner,
                PlatformInput::FileDrop(FileDropEvent::Pending {
                    position: drag_position(&inner.canvas, &event),
                }),
            );
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("dragleave", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(_event) = event.dyn_into::<DragEvent>() else {
                return;
            };
            let _ = dispatch_input(&inner, PlatformInput::FileDrop(FileDropEvent::Exited));
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_event("drop", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<DragEvent>() else {
                return;
            };
            let Some(data) = event.data_transfer() else {
                return;
            };
            event.prevent_default();
            let position = drag_position(&inner.canvas, &event);
            let text_payload = browser_drop_payload(&data, Vec::new());
            let files = browser_files(&data);
            let _ = dispatch_input(
                &inner,
                PlatformInput::FileDrop(FileDropEvent::Pending { position }),
            );
            spawn_local(async move {
                let payload = text_payload.with_files(read_browser_files(files).await);
                let _ = dispatch_input(&inner, PlatformInput::FileDrop(FileDropEvent::Exited));
                if payload.is_empty() {
                    return;
                }
                let _ = dispatch_input(
                    &inner,
                    PlatformInput::FileDrop(FileDropEvent::DataEntered {
                        position,
                        data: payload,
                    }),
                );
                let _ = dispatch_input(
                    &inner,
                    PlatformInput::FileDrop(FileDropEvent::Submit { position }),
                );
            });
        })?;
        Ok(())
    }

    fn install_webview_events(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        self.add_window_event("message", move |event| {
            let Some(inner) = weak.upgrade() else { return };
            let Ok(event) = event.dyn_into::<web_sys::MessageEvent>() else {
                return;
            };
            inner
                .webviews
                .borrow()
                .handle_message(&inner.canvas, &event);
        })?;

        let weak = Rc::downgrade(&self.0);
        self.add_window_event("scroll", move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.webviews.borrow().reposition(&inner.canvas);
            }
        })?;
        Ok(())
    }

    fn schedule_frame(&self) {
        schedule_frame_inner(&self.0);
    }
}

impl HasWindowHandle for WebWindow {
    fn window_handle(
        &self,
    ) -> std::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = RawWindowHandle::WebCanvas(WebCanvasWindowHandle::from_wasm_bindgen_0_2(
            self.0.canvas.as_ref(),
        ));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for WebWindow {
    fn display_handle(
        &self,
    ) -> std::result::Result<DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Web(WebDisplayHandle::new())) })
    }
}

impl PlatformWindow for WebWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.bounds.get()
    }
    fn is_maximized(&self) -> bool {
        self.is_fullscreen()
    }
    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }
    fn content_size(&self) -> Size<Pixels> {
        self.bounds().size
    }
    fn resize(&mut self, size: Size<Pixels>) {
        let style = self
            .0
            .surface_host
            .as_ref()
            .map_or_else(|| self.0.canvas.style(), HtmlElement::style);
        let _ = style.set_property("width", &format!("{}px", size.width.0.max(1.0)));
        let _ = style.set_property("height", &format!("{}px", size.height.0.max(1.0)));
        resize_inner(&self.0);
    }
    fn scale_factor(&self) -> f32 {
        self.0.scale_factor.get()
    }
    fn appearance(&self) -> WindowAppearance {
        browser_appearance()
    }
    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.display.clone())
    }
    fn mouse_position(&self) -> Point<Pixels> {
        self.0.mouse_position.get()
    }
    fn modifiers(&self) -> Modifiers {
        self.0.modifiers.get()
    }
    fn capslock(&self) -> Capslock {
        self.0.capslock.get()
    }
    fn set_input_handler(&mut self, handler: PlatformInputHandler) {
        let mut handler = handler;
        let caret_bounds = handler.selected_text_range(false).and_then(|selection| {
            let caret = if selection.reversed {
                selection.range.start
            } else {
                selection.range.end
            };
            handler.bounds_for_range(caret..caret)
        });
        *self.0.input_handler.borrow_mut() = Some(handler);
        if let Some(bounds) = caret_bounds {
            position_ime_input(&self.0, bounds);
        }
        self.0.ime_input.set_value("");
        let _ = self.0.ime_input.focus();
    }
    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.input_handler.borrow_mut().take()
    }
    fn prompt(
        &self,
        _level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let text = detail.map_or_else(|| msg.to_string(), |detail| format!("{msg}\n\n{detail}"));
        let accepted = web_sys::window()
            .and_then(|window| window.confirm_with_message(&text).ok())
            .unwrap_or(false);
        let index = if accepted || answers.len() <= 1 { 0 } else { 1 };
        tx.send(index).ok();
        Some(rx)
    }
    fn activate(&self) {
        // DOM focus changes are synchronous and can re-enter the blur/focus
        // handlers above. Never carry a `RefCell` guard across `focus()`.
        let has_input_handler = self.0.input_handler.borrow().is_some();
        if has_input_handler {
            let _ = self.0.ime_input.focus();
        } else {
            let _ = self.0.canvas.focus();
        }
    }
    fn is_active(&self) -> bool {
        self.0.active.get()
    }
    fn is_hovered(&self) -> bool {
        self.0.hovered.get()
    }
    fn set_title(&mut self, title: &str) {
        if let Some(host) = &self.0.surface_host {
            let _ = host.set_attribute("aria-label", title);
            let _ = host.set_attribute("data-kael-window-title", title);
        } else if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            document.set_title(title);
        }
    }
    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance) {
        let alpha = matches!(
            appearance,
            WindowBackgroundAppearance::Transparent | WindowBackgroundAppearance::Blurred
        );
        let _ = self
            .0
            .canvas
            .style()
            .set_property("background", if alpha { "transparent" } else { "#fff" });
    }
    fn set_frame_polling(&self, active: bool) {
        self.0.frame_polling.set(active);
        if active {
            self.schedule_frame();
        } else {
            cancel_scheduled_frame(&self.0);
        }
    }
    fn close(&self) {
        if self.0.closed.get() {
            return;
        }
        // Platform callbacks are allowed to synchronously update Kael and the DOM. Take
        // them out of the callback registry before invoking them so focus, resize, or
        // accessibility work triggered by the callback cannot re-enter this RefCell.
        let mut should_close_callback = self.0.callbacks.borrow_mut().should_close.take();
        let should_close = should_close_callback
            .as_mut()
            .is_none_or(|callback| callback());
        self.0.callbacks.borrow_mut().should_close = should_close_callback;
        if should_close {
            self.0.closed.set(true);
            self.0.visible.set(false);
            let _ = self.0.ime_input.blur();
            sync_window_visibility(&self.0);
            let fallback_canvas = {
                let mut registry = self.0.registry.borrow_mut();
                registry.remove(self.0.handle);
                registry.active_canvas()
            };
            detach_window_surface(&self.0);
            if let Some(canvas) = fallback_canvas {
                let _ = canvas.focus();
            }
            let callback = self.0.callbacks.borrow_mut().close.take();
            if let Some(callback) = callback {
                callback();
            }
        }
    }
    fn minimize(&self) {
        let _ = self.0.ime_input.blur();
        self.0.visible.set(false);
        sync_window_visibility(&self.0);
    }
    fn zoom(&self) {
        self.toggle_fullscreen();
    }
    fn toggle_fullscreen(&self) {
        if self.is_fullscreen() {
            if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                document.exit_fullscreen();
            }
        } else {
            if let Some(host) = &self.0.surface_host {
                let _ = host.request_fullscreen();
            } else if let Some(document) = web_sys::window().and_then(|window| window.document())
                && let Some(root) = document.document_element()
            {
                let _ = root.request_fullscreen();
            }
        }
    }
    fn is_fullscreen(&self) -> bool {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return false;
        };
        let Some(fullscreen) = document.fullscreen_element() else {
            return false;
        };
        let target: Option<web_sys::Node> = self
            .0
            .surface_host
            .as_ref()
            .map(|host| host.clone().unchecked_into())
            .or_else(|| {
                document
                    .document_element()
                    .map(|root| root.unchecked_into())
            });
        target.is_some_and(|target| fullscreen.is_same_node(Some(&target)))
    }
    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }
    fn game_input_capabilities(&self) -> GameInputCapabilities {
        GameInputCapabilities::new(
            if self.0.pointer_lock_status.get() == PointerLockStatus::Unsupported {
                GameInputAvailability::Unsupported
            } else {
                GameInputAvailability::Available
            },
            if browser_gamepads_supported() {
                GameInputAvailability::Available
            } else {
                GameInputAvailability::Unsupported
            },
        )
    }
    fn pointer_lock_status(&self) -> PointerLockStatus {
        self.0.pointer_lock_status.get()
    }
    fn request_pointer_lock(&self) -> std::result::Result<(), GameInputError> {
        if self.0.pointer_lock_status.get() == PointerLockStatus::Unsupported {
            return Err(GameInputError::new(
                GameInputErrorKind::Unsupported,
                "this browser does not expose pointer lock",
            ));
        }
        *self.0.pointer_lock_error.borrow_mut() = None;
        self.0
            .pointer_lock_status
            .set(PointerLockStatus::Requesting);
        let element: web_sys::Element = self.0.canvas.clone().unchecked_into();
        if let Err(error) = request_pointer_lock_catching(&element) {
            self.0.pointer_lock_status.set(PointerLockStatus::Unlocked);
            let error = synchronous_pointer_lock_error(error);
            *self.0.pointer_lock_error.borrow_mut() = Some(error.clone());
            schedule_frame_inner(&self.0);
            return Err(error);
        }
        Ok(())
    }
    fn exit_pointer_lock(&self) -> std::result::Result<(), GameInputError> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| {
                GameInputError::new(
                    GameInputErrorKind::Platform,
                    "browser Document is unavailable",
                )
            })?;
        let owns_lock = document
            .pointer_lock_element()
            .is_some_and(|element| element == self.0.canvas.clone().unchecked_into());
        if owns_lock {
            document.exit_pointer_lock();
        }
        Ok(())
    }
    fn pointer_lock_error(&self) -> Option<GameInputError> {
        self.0.pointer_lock_error.borrow().clone()
    }
    fn gamepads(&self) -> std::result::Result<GamepadSnapshot, GameInputError> {
        browser_gamepads()
    }
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().active = Some(callback);
    }
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().hover = Some(callback);
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
    fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
        self.0.callbacks.borrow_mut().hit_test = Some(callback);
    }
    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }
    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().appearance = Some(callback);
    }
    fn sync_webviews(&mut self, webviews: &[crate::webview::PlatformWebView]) {
        if let Err(error) = self.0.webviews.borrow_mut().sync(&self.0.canvas, webviews) {
            log::error!("failed to synchronize browser WebView surfaces: {error:#}");
        }
    }
    fn dispatch_webview_command(
        &mut self,
        command: crate::webview::PlatformWebViewCommand,
    ) -> anyhow::Result<()> {
        self.0
            .webviews
            .borrow_mut()
            .dispatch(&self.0.canvas, command)
    }
    fn print(&mut self, job: PlatformPrintJob) -> anyhow::Result<()> {
        // Browsers intentionally do not expose silent printer dispatch. Keep
        // the shared API useful by presenting the browser's print dialog.
        log::warn!("browser printing always presents the browser print dialog");
        self.0.print.borrow_mut().print(job)
    }
    fn show_print_dialog(&mut self, job: PlatformPrintJob) -> anyhow::Result<()> {
        self.0.print.borrow_mut().print(job)
    }
    fn export_scene_png(
        &self,
        scene: &Scene,
    ) -> std::result::Result<crate::Image, crate::WindowCaptureError> {
        if self.0.context_lost.get() {
            return Err(crate::WindowCaptureError::Backend(
                "browser WebGL2 context is lost".into(),
            ));
        }
        self.0
            .renderer
            .borrow_mut()
            .export_png(scene)
            .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))
    }
    fn draw(&self, scene: &Scene) {
        if self.0.context_lost.get() {
            return;
        }
        if let Err(error) = self.0.renderer.borrow_mut().draw(scene) {
            log::error!("browser WebGL2 draw failed: {error:#}");
        }
    }
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.atlas.clone()
    }
    fn gpu_specs(&self) -> Option<GpuSpecs> {
        Some(self.0.renderer.borrow().gpu_specs())
    }
    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        position_ime_input(&self.0, bounds);
    }
    fn window_controls(&self) -> WindowControls {
        WindowControls {
            fullscreen: true,
            maximize: true,
            minimize: true,
            window_menu: false,
        }
    }
    fn show(&self) {
        if self.0.closed.get() {
            return;
        }
        self.0.visible.set(true);
        sync_window_visibility(&self.0);
    }
    fn hide(&self) {
        let _ = self.0.ime_input.blur();
        self.0.visible.set(false);
        sync_window_visibility(&self.0);
    }
    fn is_visible(&self) -> bool {
        self.0.visible.get()
    }
    fn display_refresh_rate(&self) -> Option<f32> {
        self.0.display.refresh_rate()
    }
    fn update_accessibility_tree(
        &mut self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        let mut accessibility = self.0.accessibility.borrow_mut();
        if let Err(error) = accessibility.sync(tree) {
            let _ = self
                .0
                .canvas
                .set_attribute("data-kael-accessibility", "failed");
            log::error!("failed to synchronize browser accessibility tree: {error:#}");
        }
        accessibility.drain_actions()
    }
}

fn create_ime_input(document: &Document) -> Result<HtmlInputElement> {
    let input = document
        .create_element("input")
        .map_err(js_error)?
        .dyn_into::<HtmlInputElement>()
        .map_err(|_| anyhow!("failed to create browser IME input element"))?;
    input.set_type("text");
    input.set_value("");
    for (name, value) in [
        ("data-kael-ime-input", "true"),
        ("aria-hidden", "true"),
        ("autocomplete", "off"),
        ("autocapitalize", "off"),
        ("spellcheck", "false"),
        ("tabindex", "-1"),
    ] {
        input.set_attribute(name, value).map_err(js_error)?;
    }
    let style = input.style();
    for (name, value) in [
        ("position", "fixed"),
        ("left", "0px"),
        ("top", "0px"),
        ("width", "2px"),
        ("height", "2px"),
        ("margin", "0"),
        ("padding", "0"),
        ("border", "0"),
        ("outline", "0"),
        ("opacity", "0"),
        ("pointer-events", "none"),
        ("background", "transparent"),
        ("color", "transparent"),
        ("caret-color", "transparent"),
    ] {
        style.set_property(name, value).map_err(js_error)?;
    }
    document
        .body()
        .context("browser Document has no body for Kael's IME bridge")?
        .append_child(&input)
        .map_err(js_error)?;
    Ok(input)
}

fn position_ime_input(inner: &WebWindowInner, bounds: Bounds<Pixels>) {
    let canvas = inner.canvas.get_bounding_client_rect();
    let left = canvas.left() + f64::from(bounds.origin.x.0);
    let top = canvas.top() + f64::from((bounds.origin.y + bounds.size.height).0);
    let style = inner.ime_input.style();
    let _ = style.set_property("left", &format!("{}px", left.max(0.0)));
    let _ = style.set_property("top", &format!("{}px", top.max(0.0)));
}

fn set_surface_hidden(inner: &WebWindowInner, hidden: bool) {
    inner.canvas.set_hidden(hidden);
    if let Some(host) = &inner.surface_host {
        host.set_hidden(hidden);
    }
}

fn sync_window_visibility(inner: &Rc<WebWindowInner>) {
    let visible = inner.visible.get() && !inner.application_hidden.get() && !inner.closed.get();
    set_surface_hidden(inner, !visible);
    inner.accessibility.borrow_mut().set_visible(visible);
    inner.webviews.borrow().set_parent_visible(visible);
    if visible {
        schedule_frame_inner(inner);
    } else {
        cancel_scheduled_frame(inner);
    }
}

fn detach_window_surface(inner: &WebWindowInner) {
    if let Some(host) = &inner.surface_host {
        host.remove();
    } else {
        let _ = inner.canvas.remove_attribute("data-kael-window-surface-id");
        let _ = inner.canvas.remove_attribute("data-kael-window-primary");
    }
}

fn resize_inner(inner: &Rc<WebWindowInner>) {
    let rect = inner.canvas.get_bounding_client_rect();
    let logical = size(
        px(rect.width().max(1.0) as f32),
        px(rect.height().max(1.0) as f32),
    );
    let scale = web_sys::window().map_or(1.0, |window| window.device_pixel_ratio().max(0.1)) as f32;
    let device = device_size(logical, scale);
    if inner.canvas.width() != device.width.0 as u32
        || inner.canvas.height() != device.height.0 as u32
    {
        inner.canvas.set_width(device.width.0 as u32);
        inner.canvas.set_height(device.height.0 as u32);
        inner.renderer.borrow_mut().resize(device);
    }
    let mut bounds = inner.bounds.get();
    let changed = bounds.size != logical || inner.scale_factor.get() != scale;
    bounds.size = logical;
    inner.bounds.set(bounds);
    inner.scale_factor.set(scale);
    if changed {
        let mut callback = inner.callbacks.borrow_mut().resize.take();
        if let Some(callback_fn) = callback.as_mut() {
            callback_fn(logical, scale);
        }
        inner.callbacks.borrow_mut().resize = callback;
    }
    inner.webviews.borrow().reposition(&inner.canvas);
}

fn browser_document_is_visible() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_none_or(|document| document.visibility_state() == web_sys::VisibilityState::Visible)
}

fn schedule_frame_inner(inner: &Rc<WebWindowInner>) {
    if !inner.frame_polling.get()
        || !inner.visible.get()
        || inner.application_hidden.get()
        || inner.context_lost.get()
        || !browser_document_is_visible()
        || inner.raf_handle.get().is_some()
    {
        return;
    }
    let callback_ref = inner.raf_callback.borrow();
    let Some(callback) = callback_ref.as_ref() else {
        return;
    };
    if let Some(browser) = web_sys::window()
        && let Ok(handle) = browser.request_animation_frame(callback.as_ref().unchecked_ref())
    {
        inner.raf_handle.set(Some(handle));
    }
}

fn cancel_scheduled_frame(inner: &WebWindowInner) {
    if let Some(handle) = inner.raf_handle.take()
        && let Some(browser) = web_sys::window()
    {
        let _ = browser.cancel_animation_frame(handle);
    }
}

fn device_size(logical: Size<Pixels>, scale: f32) -> Size<DevicePixels> {
    size(
        DevicePixels((logical.width.0 * scale).round().max(1.0) as i32),
        DevicePixels((logical.height.0 * scale).round().max(1.0) as i32),
    )
}

fn dispatch_input(inner: &Rc<WebWindowInner>, input: PlatformInput) -> DispatchEventResult {
    let mut callback = inner.callbacks.borrow_mut().input.take();
    let result = callback.as_mut().map_or(
        DispatchEventResult {
            propagate: true,
            default_prevented: false,
        },
        |callback| callback(input),
    );
    inner.callbacks.borrow_mut().input = callback;
    result
}

const MAX_BROWSER_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BROWSER_DROP_BYTES: u64 = 512 * 1024 * 1024;

fn drag_position(canvas: &HtmlCanvasElement, event: &DragEvent) -> Point<Pixels> {
    let rect = canvas.get_bounding_client_rect();
    point(
        px(event.client_x() as f32 - rect.left() as f32),
        px(event.client_y() as f32 - rect.top() as f32),
    )
}

fn browser_files(data: &DataTransfer) -> Vec<File> {
    let Some(files) = data.files() else {
        return Vec::new();
    };
    (0..files.length())
        .filter_map(|index| files.get(index))
        .collect()
}

fn browser_file_metadata(data: &DataTransfer) -> Vec<ExternalFile> {
    browser_files(data)
        .into_iter()
        .map(|file| {
            let mime = nonempty_string(file.type_());
            ExternalFile::new(file.name(), mime, Vec::new())
        })
        .collect()
}

fn browser_drop_payload(data: &DataTransfer, files: Vec<ExternalFile>) -> ExternalDropData {
    let plain = data.get_data("text/plain").unwrap_or_default();
    let uri_list = data.get_data("text/uri-list").unwrap_or_default();
    let mut payload = if plain.is_empty() {
        ExternalDropData::new()
    } else {
        ExternalDropData::from_plain_text(plain)
    }
    .with_files(files);
    if !uri_list.is_empty() {
        let uris = ExternalDropData::from_uri_list(&uri_list);
        payload = payload.with_urls(uris.urls().iter().cloned());
    }
    payload
}

pub(super) async fn read_browser_files(files: Vec<File>) -> Vec<ExternalFile> {
    let mut total_bytes = 0u64;
    let mut output = Vec::with_capacity(files.len());
    for file in files {
        let name = file.name();
        let mime = nonempty_string(file.type_());
        let byte_len = file.size().max(0.0) as u64;
        let next_total = total_bytes.saturating_add(byte_len);
        if byte_len > MAX_BROWSER_FILE_BYTES || next_total > MAX_BROWSER_DROP_BYTES {
            output.push(ExternalFile::unavailable(
                name,
                mime,
                "browser file exceeds Kael's bounded intake limit",
            ));
            continue;
        }
        let Ok(buffer) = JsFuture::from(file.array_buffer()).await else {
            output.push(ExternalFile::unavailable(
                name,
                mime,
                "browser could not read the selected file",
            ));
            continue;
        };
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        output.push(ExternalFile::new(name, mime, bytes));
    }
    output
}

fn nonempty_string(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserClipboardShortcut {
    Copy,
    Cut,
    Paste,
}

fn browser_clipboard_shortcut(keystroke: &Keystroke) -> Option<BrowserClipboardShortcut> {
    let accelerator = keystroke.modifiers.control || keystroke.modifiers.platform;
    if !accelerator || keystroke.modifiers.alt {
        return None;
    }
    match keystroke.key.as_str() {
        "c" if !keystroke.modifiers.shift => Some(BrowserClipboardShortcut::Copy),
        "x" if !keystroke.modifiers.shift => Some(BrowserClipboardShortcut::Cut),
        "v" => Some(BrowserClipboardShortcut::Paste),
        _ => None,
    }
}

fn handle_clipboard_write_event(inner: &Rc<WebWindowInner>, event: Event) {
    let Ok(event) = event.dyn_into::<ClipboardEvent>() else {
        return;
    };
    let Some(expected_revision) = inner.pending_clipboard_write.take() else {
        return;
    };
    let item = {
        let clipboard = inner.clipboard.lock();
        if clipboard.revision() != expected_revision {
            return;
        }
        clipboard.snapshot()
    };
    let Some(item) = item else { return };
    let Some(data) = event.clipboard_data() else {
        return;
    };
    let mut wrote = false;
    if let Some(text) = item.text()
        && data.set_data("text/plain", &text).is_ok()
    {
        wrote = true;
    }
    if let Some(html) = item.html()
        && data.set_data("text/html", &html).is_ok()
    {
        wrote = true;
    }
    if wrote {
        event.prevent_default();
    }
}

fn handle_clipboard_paste_event(inner: &Rc<WebWindowInner>, event: Event) {
    let Ok(event) = event.dyn_into::<ClipboardEvent>() else {
        return;
    };
    let Some(data) = event.clipboard_data() else {
        return;
    };
    let payload = match browser_clipboard_paste_payload(&data) {
        Ok(Some(payload)) => payload,
        Ok(None) => return,
        Err(error) => {
            event.prevent_default();
            inner.pending_paste.borrow_mut().take();
            inner.ime_input.set_value("");
            log::warn!("rejected browser clipboard paste: {}", error.message());
            return;
        }
    };
    let BrowserClipboardPastePayload {
        text_item,
        text_bytes,
        image_files,
    } = payload;
    if text_item.is_none() && image_files.is_empty() {
        return;
    }

    event.prevent_default();
    inner.ime_input.set_value("");
    let keystroke = inner
        .pending_paste
        .borrow_mut()
        .take()
        .unwrap_or_else(browser_paste_keystroke);
    if image_files.is_empty() {
        if let Some(item) = text_item {
            inner.clipboard.lock().store(item);
            dispatch_paste(inner, keystroke);
        }
        return;
    }

    let inner = inner.clone();
    spawn_local(async move {
        let Ok(mut actual_budget) = BrowserClipboardBudget::from_total_bytes(text_bytes) else {
            return;
        };
        let mut entries = text_item
            .into_iter()
            .flat_map(ClipboardItem::into_entries)
            .collect::<Vec<_>>();
        for (file, format) in image_files {
            let read_bytes = actual_budget.bounded_image_read_bytes();
            let Ok(blob) = file.slice_with_f64_and_f64(0.0, read_bytes as f64) else {
                continue;
            };
            let Ok(buffer) = JsFuture::from(blob.array_buffer()).await else {
                continue;
            };
            let view = js_sys::Uint8Array::new(&buffer);
            let byte_len = u64::from(view.length());
            if let Err(error) = actual_budget.try_add(BrowserClipboardItemKind::Image, byte_len) {
                log::warn!("rejected browser clipboard paste: {}", error.message());
                return;
            }
            let bytes = view.to_vec();
            if !bytes.is_empty() {
                entries.push(ClipboardEntry::Image(ClipboardImage::from_bytes(
                    format, bytes,
                )));
            }
        }
        if entries.iter().any(|entry| match entry {
            ClipboardEntry::String(string) => string.validate().is_err(),
            ClipboardEntry::Image(image) => image.validate().is_err(),
        }) {
            log::warn!(
                "rejected browser clipboard paste: {}",
                BrowserClipboardLimitError::InvalidPayload.message()
            );
            return;
        }
        let Ok(item) = ClipboardItem::from_entries(entries) else {
            return;
        };
        inner.clipboard.lock().store(item);
        dispatch_paste(&inner, keystroke);
    });
}

struct BrowserClipboardPastePayload {
    text_item: Option<ClipboardItem>,
    text_bytes: u64,
    image_files: Vec<(File, ClipboardImageFormat)>,
}

fn browser_clipboard_paste_payload(
    data: &DataTransfer,
) -> std::result::Result<Option<BrowserClipboardPastePayload>, BrowserClipboardLimitError> {
    validate_browser_clipboard_item_count(data.items().length() as usize)?;
    validate_browser_clipboard_item_count(data.types().length() as usize)?;

    let mut budget = BrowserClipboardBudget::default();
    let plain = bounded_clipboard_string(
        data,
        "text/plain",
        BrowserClipboardItemKind::PlainText,
        &mut budget,
    )?;
    let html = bounded_clipboard_string(
        data,
        "text/html",
        BrowserClipboardItemKind::Html,
        &mut budget,
    )?;
    let uri_list = bounded_clipboard_string(
        data,
        "text/uri-list",
        BrowserClipboardItemKind::UriList,
        &mut budget,
    )?;
    let fallback = if plain.is_empty() { uri_list } else { plain };
    let text_item = if fallback.is_empty() {
        None
    } else if html.is_empty() {
        Some(
            ClipboardItem::builder()
                .text(fallback)
                .build()
                .map_err(|_| BrowserClipboardLimitError::InvalidPayload)?,
        )
    } else {
        Some(
            ClipboardItem::builder()
                .html(fallback, html)
                .map_err(|_| BrowserClipboardLimitError::InvalidPayload)?
                .build()
                .map_err(|_| BrowserClipboardLimitError::InvalidPayload)?,
        )
    };
    let text_bytes = budget.total_bytes();
    let image_files = clipboard_image_files(data, &mut budget)?;
    if text_item.is_none() && image_files.is_empty() {
        Ok(None)
    } else {
        Ok(Some(BrowserClipboardPastePayload {
            text_item,
            text_bytes,
            image_files,
        }))
    }
}

fn bounded_clipboard_string(
    data: &DataTransfer,
    mime_type: &str,
    kind: BrowserClipboardItemKind,
    budget: &mut BrowserClipboardBudget,
) -> std::result::Result<String, BrowserClipboardLimitError> {
    let value = data.get_data(mime_type).unwrap_or_default();
    if !value.is_empty() {
        budget.try_add(kind, value.len() as u64)?;
    }
    Ok(value)
}

fn clipboard_image_files(
    data: &DataTransfer,
    budget: &mut BrowserClipboardBudget,
) -> std::result::Result<Vec<(File, ClipboardImageFormat)>, BrowserClipboardLimitError> {
    let Some(files) = data.files() else {
        return Ok(Vec::new());
    };
    validate_browser_clipboard_item_count(files.length() as usize)?;
    let mut images = Vec::with_capacity((files.length() as usize).min(MAX_BROWSER_CLIPBOARD_ITEMS));
    for index in 0..files.length() {
        let Some(file) = files.get(index) else {
            continue;
        };
        let Some(format) = ClipboardImageFormat::from_mime_type(&file.type_()) else {
            continue;
        };
        let size = file.size();
        if !size.is_finite() || size < 0.0 || size > u64::MAX as f64 {
            return Err(BrowserClipboardLimitError::InvalidItemSize);
        }
        budget.try_add(BrowserClipboardItemKind::Image, size as u64)?;
        images.push((file, format));
    }
    Ok(images)
}

fn browser_paste_keystroke() -> Keystroke {
    Keystroke {
        modifiers: Modifiers {
            control: true,
            ..Modifiers::default()
        },
        key: "v".into(),
        key_char: None,
    }
}

fn dispatch_paste(inner: &Rc<WebWindowInner>, keystroke: Keystroke) {
    let _ = dispatch_input(
        inner,
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: false,
        }),
    );
}

fn handle_keydown_event(inner: &Rc<WebWindowInner>, event: Event) {
    let Ok(event) = event.dyn_into::<KeyboardEvent>() else {
        return;
    };
    if event.is_composing() || matches!(event.key().as_str(), "Process" | "Dead") {
        return;
    }
    let keystroke = keystroke(&event);
    inner.modifiers.set(keystroke.modifiers);
    inner.capslock.set(Capslock {
        on: event.get_modifier_state("CapsLock"),
    });
    let clipboard_shortcut = browser_clipboard_shortcut(&keystroke);
    if clipboard_shortcut == Some(BrowserClipboardShortcut::Paste) {
        // A paste event exposes the browser's clipboard payload synchronously.
        // Delay Kael's paste action until that event refreshes the shared mirror.
        inner.pending_paste.replace(Some(keystroke));
        return;
    }
    let clipboard_revision = clipboard_shortcut.map(|_| inner.clipboard.lock().revision());
    let result = dispatch_input(
        inner,
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: event.repeat(),
        }),
    );
    if matches!(
        clipboard_shortcut,
        Some(BrowserClipboardShortcut::Copy | BrowserClipboardShortcut::Cut)
    ) {
        let revision = inner.clipboard.lock().revision();
        if clipboard_revision.is_some_and(|before| revision != before) {
            inner.pending_clipboard_write.set(Some(revision));
        }
        // Let the browser emit `copy`/`cut`; that event is the synchronous,
        // permission-safe fallback when Async Clipboard is unavailable.
        return;
    }
    if should_prevent_default(result) {
        event.prevent_default();
    }
}

fn handle_keyup_event(inner: &Rc<WebWindowInner>, event: Event) {
    let Ok(event) = event.dyn_into::<KeyboardEvent>() else {
        return;
    };
    if event.is_composing() || event.key() == "Process" {
        return;
    }
    let keystroke = keystroke(&event);
    inner.modifiers.set(keystroke.modifiers);
    inner.capslock.set(Capslock {
        on: event.get_modifier_state("CapsLock"),
    });
    if should_prevent_default(dispatch_input(
        inner,
        PlatformInput::KeyUp(KeyUpEvent { keystroke }),
    )) {
        event.prevent_default();
    }
}

fn set_active_inner(inner: &Rc<WebWindowInner>, active: bool) {
    if active {
        inner.registry.borrow_mut().activate(inner.handle);
    } else {
        inner.registry.borrow_mut().deactivate(inner.handle);
    }
    if inner.active.replace(active) != active {
        invoke_bool_callback(inner, active, true);
    }
}

fn window_control_area(inner: &Rc<WebWindowInner>) -> Option<WindowControlArea> {
    let mut callback = inner.callbacks.borrow_mut().hit_test.take();
    let result = callback.as_mut().and_then(|callback| callback());
    inner.callbacks.borrow_mut().hit_test = callback;
    result
}

fn handle_window_control_pointer_down(inner: &Rc<WebWindowInner>, event: &DomPointerEvent) -> bool {
    let Some(area) = window_control_area(inner) else {
        return false;
    };
    let window = WebWindow(inner.clone());
    match area {
        WindowControlArea::Close => window.close(),
        WindowControlArea::Max => window.toggle_fullscreen(),
        WindowControlArea::Min => window.minimize(),
        WindowControlArea::Drag => {
            let Some(_) = &inner.surface_host else {
                return false;
            };
            if !inner.movable {
                return false;
            }
            let bounds = inner.bounds.get();
            inner.window_drag.set(Some(BrowserWindowDrag {
                pointer_id: event.pointer_id(),
                start_client_x: event.client_x(),
                start_client_y: event.client_y(),
                start_left: bounds.origin.x.0,
                start_top: bounds.origin.y.0,
            }));
        }
    }
    true
}

fn move_browser_window_drag(inner: &Rc<WebWindowInner>, event: &DomPointerEvent) -> bool {
    let Some(drag) = inner.window_drag.get() else {
        return false;
    };
    if drag.pointer_id != event.pointer_id() {
        return false;
    }
    let Some(host) = &inner.surface_host else {
        inner.window_drag.set(None);
        return false;
    };
    let mut left = drag.start_left + (event.client_x() - drag.start_client_x) as f32;
    let mut top = drag.start_top + (event.client_y() - drag.start_client_y) as f32;
    if let Some(browser) = web_sys::window() {
        let viewport_width = browser
            .inner_width()
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or_default() as f32;
        let viewport_height = browser
            .inner_height()
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or_default() as f32;
        let rect = host.get_bounding_client_rect();
        let min_left = 32.0 - rect.width() as f32;
        let max_left = (viewport_width - 32.0).max(0.0);
        left = if min_left <= max_left {
            left.clamp(min_left, max_left)
        } else {
            0.0
        };
        top = top.clamp(0.0, (viewport_height - 32.0).max(0.0));
    }
    let style = host.style();
    let _ = style.set_property("left", &format!("{left}px"));
    let _ = style.set_property("top", &format!("{top}px"));
    let mut bounds = inner.bounds.get();
    bounds.origin = point(px(left), px(top));
    inner.bounds.set(bounds);
    let mut callback = inner.callbacks.borrow_mut().moved.take();
    if let Some(callback) = callback.as_mut() {
        callback();
    }
    inner.callbacks.borrow_mut().moved = callback;
    inner.webviews.borrow().reposition(&inner.canvas);
    true
}

fn canvas_has_dom_focus(inner: &WebWindowInner) -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .is_some_and(|element| {
            let canvas_node: &web_sys::Node = inner.canvas.unchecked_ref();
            element.is_same_node(Some(canvas_node))
        })
}

fn should_prevent_default(result: DispatchEventResult) -> bool {
    !result.propagate || result.default_prevented
}

fn invoke_bool_callback(inner: &Rc<WebWindowInner>, value: bool, active: bool) {
    let mut callback = if active {
        inner.callbacks.borrow_mut().active.take()
    } else {
        inner.callbacks.borrow_mut().hover.take()
    };
    if let Some(callback_fn) = callback.as_mut() {
        callback_fn(value);
    }
    if active {
        inner.callbacks.borrow_mut().active = callback;
    } else {
        inner.callbacks.borrow_mut().hover = callback;
    }
}

const MAX_COALESCED_POINTER_SAMPLES: usize = 256;

fn pointer_position(canvas: &HtmlCanvasElement, event: &DomPointerEvent) -> Point<Pixels> {
    let rect = canvas.get_bounding_client_rect();
    point(
        px(event.client_x() as f32 - rect.left() as f32),
        px(event.client_y() as f32 - rect.top() as f32),
    )
}

fn pointer_modifiers(event: &DomPointerEvent) -> Modifiers {
    Modifiers {
        control: event.ctrl_key(),
        alt: event.alt_key(),
        shift: event.shift_key(),
        platform: event.meta_key(),
        function: false,
    }
}

fn browser_pointer_event(
    canvas: &HtmlCanvasElement,
    event: &DomPointerEvent,
    phase: PointerPhase,
    include_coalesced: bool,
) -> PointerInputEvent {
    let position = pointer_position(canvas, event);
    let pressure = finite_clamped(event.pressure(), 0.0, 1.0);
    let tangential_pressure = finite_clamped(event.tangential_pressure(), -1.0, 1.0);
    let tilt_x = (event.tilt_x() as f32).clamp(-90.0, 90.0);
    let tilt_y = (event.tilt_y() as f32).clamp(-90.0, 90.0);
    let twist = (event.twist() as f32).rem_euclid(360.0);
    let width = px(finite_non_negative(event.width() as f32));
    let height = px(finite_non_negative(event.height() as f32));
    let timestamp_ms = finite_timestamp(event.time_stamp());
    let current_sample = PointerSample {
        position,
        movement: point(px(event.movement_x() as f32), px(event.movement_y() as f32)),
        pressure,
        tangential_pressure,
        tilt_x,
        tilt_y,
        twist,
        width,
        height,
        timestamp_ms,
    };
    let coalesced = if include_coalesced {
        coalesced_pointer_samples(canvas, event, current_sample)
    } else {
        Vec::new()
    };

    PointerInputEvent {
        phase,
        pointer_id: PointerId::new(i64::from(event.pointer_id())),
        pointer_type: match event.pointer_type().as_str() {
            "mouse" => PointerType::Mouse,
            "touch" => PointerType::Touch,
            "pen" => PointerType::Pen,
            _ => PointerType::Unknown,
        },
        position,
        movement: point(px(event.movement_x() as f32), px(event.movement_y() as f32)),
        button: matches!(
            phase,
            PointerPhase::Down | PointerPhase::Up | PointerPhase::Cancel
        )
        .then(|| optional_mouse_button(event.button()))
        .flatten(),
        buttons: PointerButtons::from_bits_retain(event.buttons()),
        modifiers: pointer_modifiers(event),
        click_count: usize::try_from(event.detail().max(1)).unwrap_or(1),
        is_primary: event.is_primary(),
        pressure,
        tangential_pressure,
        tilt_x,
        tilt_y,
        twist,
        width,
        height,
        timestamp_ms,
        coalesced,
    }
}

fn coalesced_pointer_samples(
    canvas: &HtmlCanvasElement,
    event: &DomPointerEvent,
    current: PointerSample,
) -> Vec<PointerSample> {
    let Ok(method) = js_sys::Reflect::get(event.as_ref(), &JsValue::from_str("getCoalescedEvents"))
    else {
        return Vec::new();
    };
    let Ok(method) = method.dyn_into::<js_sys::Function>() else {
        return Vec::new();
    };
    let Ok(values) = method.call0(event.as_ref()) else {
        return Vec::new();
    };
    let values = js_sys::Array::from(&values);
    values
        .iter()
        .take(MAX_COALESCED_POINTER_SAMPLES)
        .filter_map(|value| value.dyn_into::<DomPointerEvent>().ok())
        .map(|sample| PointerSample {
            position: pointer_position(canvas, &sample),
            movement: point(
                px(sample.movement_x() as f32),
                px(sample.movement_y() as f32),
            ),
            pressure: finite_clamped(sample.pressure(), 0.0, 1.0),
            tangential_pressure: finite_clamped(sample.tangential_pressure(), -1.0, 1.0),
            tilt_x: (sample.tilt_x() as f32).clamp(-90.0, 90.0),
            tilt_y: (sample.tilt_y() as f32).clamp(-90.0, 90.0),
            twist: (sample.twist() as f32).rem_euclid(360.0),
            width: px(finite_non_negative(sample.width() as f32)),
            height: px(finite_non_negative(sample.height() as f32)),
            timestamp_ms: finite_timestamp(sample.time_stamp()),
        })
        .filter(|sample| *sample != current)
        .collect()
}

fn finite_clamped(value: f32, minimum: f32, maximum: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        minimum.max(0.0)
    }
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn finite_timestamp(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn wheel_modifiers(event: &WheelEvent) -> Modifiers {
    Modifiers {
        control: event.ctrl_key(),
        alt: event.alt_key(),
        shift: event.shift_key(),
        platform: event.meta_key(),
        function: false,
    }
}

fn optional_mouse_button(button: i16) -> Option<MouseButton> {
    match button {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Middle),
        2 => Some(MouseButton::Right),
        3 => Some(MouseButton::Navigate(NavigationDirection::Back)),
        4 => Some(MouseButton::Navigate(NavigationDirection::Forward)),
        _ => None,
    }
}

fn keystroke(event: &KeyboardEvent) -> Keystroke {
    let raw = event.key();
    let key = match raw.as_str() {
        " " => "space".into(),
        "ArrowUp" => "up".into(),
        "ArrowDown" => "down".into(),
        "ArrowLeft" => "left".into(),
        "ArrowRight" => "right".into(),
        "Escape" => "escape".into(),
        "Enter" => "enter".into(),
        "Backspace" => "backspace".into(),
        "Delete" => "delete".into(),
        "Tab" => "tab".into(),
        "Home" => "home".into(),
        "End" => "end".into(),
        "PageUp" => "pageup".into(),
        "PageDown" => "pagedown".into(),
        _ => raw.to_lowercase(),
    };
    let modifiers = Modifiers {
        control: event.ctrl_key(),
        alt: event.alt_key(),
        shift: event.shift_key(),
        platform: event.meta_key(),
        function: false,
    };
    let key_char =
        (raw.chars().count() == 1 && !modifiers.control && !modifiers.platform).then_some(raw);
    Keystroke {
        modifiers,
        key,
        key_char,
    }
}

pub(crate) fn browser_appearance() -> WindowAppearance {
    web_sys::window()
        .and_then(|window| {
            window
                .match_media("(prefers-color-scheme: dark)")
                .ok()
                .flatten()
        })
        .filter(|query| query.matches())
        .map_or(WindowAppearance::Light, |_| WindowAppearance::Dark)
}

fn browser_pointer_lock_supported(canvas: &HtmlCanvasElement, document: &Document) -> bool {
    js_sys::Reflect::has(canvas.as_ref(), &JsValue::from_str("requestPointerLock")).unwrap_or(false)
        && js_sys::Reflect::has(document.as_ref(), &JsValue::from_str("exitPointerLock"))
            .unwrap_or(false)
}

fn synchronous_pointer_lock_error(error: JsValue) -> GameInputError {
    let name = js_sys::Reflect::get(&error, &JsValue::from_str("name"))
        .ok()
        .and_then(|name| name.as_string());
    let (kind, message) = match name.as_deref() {
        Some("NotAllowedError") => (
            GameInputErrorKind::UserGestureRequired,
            "browser rejected pointer lock synchronously; request it from a trusted user activation",
        ),
        Some("NotSupportedError") => (
            GameInputErrorKind::Unsupported,
            "this browser context does not support pointer lock",
        ),
        Some("SecurityError") => (
            GameInputErrorKind::Rejected,
            "browser security policy rejected pointer lock",
        ),
        _ => (
            GameInputErrorKind::Rejected,
            "browser rejected pointer lock synchronously",
        ),
    };
    GameInputError::new(kind, message)
}

fn request_pointer_lock_catching(element: &web_sys::Element) -> std::result::Result<(), JsValue> {
    let method = js_sys::Reflect::get(element.as_ref(), &JsValue::from_str("requestPointerLock"))?;
    let method = method.dyn_into::<js_sys::Function>()?;
    method.call0(element.as_ref()).map(|_| ())
}

fn browser_gamepads_supported() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(
            window.navigator().as_ref(),
            &JsValue::from_str("getGamepads"),
        )
        .unwrap_or(false)
    })
}

fn browser_gamepads() -> std::result::Result<GamepadSnapshot, GameInputError> {
    if !browser_gamepads_supported() {
        return Err(GameInputError::new(
            GameInputErrorKind::Unsupported,
            "this browser does not expose the Gamepad API",
        ));
    }
    let window = web_sys::window().ok_or_else(|| {
        GameInputError::new(
            GameInputErrorKind::Platform,
            "browser Window is unavailable",
        )
    })?;
    let values = window.navigator().get_gamepads().map_err(|error| {
        GameInputError::new(
            GameInputErrorKind::Platform,
            format!("browser Gamepad API failed: {}", js_error(error)),
        )
    })?;
    let gamepads = values
        .iter()
        .filter(|value| !value.is_null() && !value.is_undefined())
        // `Gamepad` is an unconstructable browser interface on several engines,
        // so it is intentionally treated as a structural platform object.
        .map(JsValue::unchecked_into::<BrowserGamepad>)
        .filter(|gamepad| gamepad.connected())
        .take(crate::MAX_GAMEPADS)
        .map(|gamepad| {
            let axes = gamepad
                .axes()
                .iter()
                .take(crate::MAX_GAMEPAD_AXES)
                .map(|value| {
                    crate::game_input::finite_clamped(value.as_f64().unwrap_or_default(), -1.0, 1.0)
                })
                .collect();
            let buttons = gamepad
                .buttons()
                .iter()
                .take(crate::MAX_GAMEPAD_BUTTONS)
                .filter(|value| !value.is_null() && !value.is_undefined())
                .map(JsValue::unchecked_into::<web_sys::GamepadButton>)
                .map(|button| {
                    GamepadButtonState::sanitized(
                        button.value(),
                        button.pressed(),
                        button.touched(),
                    )
                })
                .collect();
            GamepadState {
                index: gamepad.index(),
                id: crate::game_input::bounded_id(&gamepad.id()),
                mapping: if gamepad.mapping() == GamepadMappingType::Standard {
                    GamepadMapping::Standard
                } else {
                    GamepadMapping::Raw
                },
                timestamp_ms: if gamepad.timestamp().is_finite() {
                    gamepad.timestamp().max(0.0)
                } else {
                    0.0
                },
                axes,
                buttons,
            }
        })
        .collect();
    Ok(GamepadSnapshot {
        gamepads,
        events_drained: 0,
        event_budget_exhausted: false,
    })
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

impl Drop for WebWindowInner {
    fn drop(&mut self) {
        self.closed.set(true);
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let owns_lock = document
                .pointer_lock_element()
                .is_some_and(|element| element == self.canvas.clone().unchecked_into());
            if owns_lock {
                document.exit_pointer_lock();
            }
        }
        if let Ok(mut registry) = self.registry.try_borrow_mut() {
            registry.remove(self.handle);
        }
        for listener in self.event_callbacks.get_mut().drain(..) {
            let _ = listener.target.remove_event_listener_with_callback(
                &listener.name,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        if let Some(observer) = self.resize_observer.get_mut().take() {
            observer.disconnect();
        }
        if let Some(handle) = self.raf_handle.take()
            && let Some(browser) = web_sys::window()
        {
            let _ = browser.cancel_animation_frame(handle);
        }
        self.ime_input.remove();
        detach_window_surface(self);
        self.renderer.get_mut().destroy();
    }
}
