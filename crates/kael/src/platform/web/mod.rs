mod accessibility;
mod atlas;
mod dispatcher;
mod print;
mod renderer;
#[cfg(feature = "screen-capture")]
mod screen_capture;
mod text_system;
mod webview;
mod window;

use crate::{
    AnyWindowHandle, BackgroundExecutor, Bounds, ClipboardEntry, ClipboardItem, CursorStyle,
    DisplayId, DummyKeyboardMapper, ExternalFile, ForegroundExecutor, Keymap, Menu, MenuItem,
    PathPromptOptions, Pixels, Platform, PlatformDisplay, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, Task, WindowAppearance, WindowParams,
    platform::web_scene_math::RefreshRateEstimator, point, px,
};
use anyhow::{Context as _, Result, anyhow};
use dispatcher::WebDispatcher;
use futures::channel::oneshot;
use parking_lot::Mutex;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
use text_system::WebTextSystem;
use uuid::Uuid;
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Blob, BlobPropertyBag, ClipboardItem as BrowserClipboardItem, Event, EventTarget,
    HtmlCanvasElement, HtmlElement, HtmlInputElement,
};
use window::{WebWindow, browser_appearance, read_browser_files};

struct BrowserFilePicker {
    input: HtmlInputElement,
    callbacks: Vec<(&'static str, Closure<dyn FnMut(Event)>)>,
}

struct BrowserPlatformListener {
    target: EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for BrowserPlatformListener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}

struct WebWindowRegistryEntry {
    handle: AnyWindowHandle,
    canvas: HtmlCanvasElement,
    host: Option<HtmlElement>,
    surface_id: String,
    sync_visibility: Rc<dyn Fn()>,
}

#[derive(Default)]
pub(super) struct WebWindowRegistry {
    entries: Vec<WebWindowRegistryEntry>,
    active: Option<AnyWindowHandle>,
    next_surface_id: u64,
}

pub(super) fn window_layer_id(canvas: &HtmlCanvasElement, base: &str) -> String {
    if canvas.get_attribute("data-kael-window-primary").as_deref() == Some("true") {
        base.to_owned()
    } else {
        let surface_id = canvas
            .get_attribute("data-kael-window-surface-id")
            .unwrap_or_else(|| "unknown".to_owned());
        format!("{base}-{surface_id}")
    }
}

impl WebWindowRegistry {
    fn allocate_surface_id(&mut self) -> u64 {
        self.next_surface_id = self.next_surface_id.wrapping_add(1).max(1);
        self.next_surface_id
    }

    pub(super) fn register(
        &mut self,
        handle: AnyWindowHandle,
        canvas: HtmlCanvasElement,
        host: Option<HtmlElement>,
        sync_visibility: Rc<dyn Fn()>,
        activate: bool,
    ) {
        let surface_id = canvas
            .get_attribute("data-kael-window-surface-id")
            .unwrap_or_else(|| "unknown".to_owned());
        self.entries.retain(|entry| entry.handle != handle);
        self.entries.push(WebWindowRegistryEntry {
            handle,
            canvas,
            host,
            surface_id,
            sync_visibility,
        });
        if activate {
            self.active = Some(handle);
            self.raise(handle);
        } else {
            self.restack();
        }
    }

    pub(super) fn activate(&mut self, handle: AnyWindowHandle) {
        if self.entries.iter().any(|entry| entry.handle == handle) {
            self.active = Some(handle);
            self.raise(handle);
        }
    }

    pub(super) fn deactivate(&mut self, handle: AnyWindowHandle) {
        if self.active == Some(handle) {
            self.active = None;
        }
    }

    pub(super) fn remove(&mut self, handle: AnyWindowHandle) {
        self.entries.retain(|entry| entry.handle != handle);
        if self.active == Some(handle) {
            self.active = self.entries.last().map(|entry| entry.handle);
        } else if self.active.is_none() {
            // A focused surface emits `blur` before its semantic close path,
            // so `deactivate` may already have cleared the active handle. Keep
            // the retained application window model usable by promoting the
            // topmost remaining surface after removal.
            self.active = self.entries.last().map(|entry| entry.handle);
        }
        self.restack();
    }

    fn raise(&mut self, handle: AnyWindowHandle) {
        let Some(index) = self.entries.iter().position(|entry| entry.handle == handle) else {
            return;
        };
        let entry = self.entries.remove(index);
        self.entries.push(entry);
        self.restack();
    }

    fn restack(&self) {
        for (index, entry) in self.entries.iter().enumerate() {
            let base_z = 10_000usize.saturating_add(index.saturating_mul(10));
            if let Some(host) = &entry.host {
                let _ = host.style().set_property("z-index", &base_z.to_string());
            }
            let Some(document) = web_sys::window().and_then(|window| window.document()) else {
                continue;
            };
            let suffix = entry
                .host
                .is_some()
                .then(|| format!("-{}", entry.surface_id))
                .unwrap_or_default();
            for (base_id, offset) in [
                ("kael-webview-layer", 5usize),
                ("kael-accessibility-layer", 6usize),
            ] {
                let Some(layer) = document
                    .get_element_by_id(&format!("{base_id}{suffix}"))
                    .and_then(|element| element.dyn_into::<HtmlElement>().ok())
                else {
                    continue;
                };
                let _ = layer
                    .style()
                    .set_property("z-index", &base_z.saturating_add(offset).to_string());
            }
        }
        self.publish_state();
    }

    fn active_canvas(&self) -> Option<HtmlCanvasElement> {
        let handle = self.active?;
        self.entries
            .iter()
            .find(|entry| entry.handle == handle)
            .map(|entry| entry.canvas.clone())
    }

    fn handles(&self) -> Vec<AnyWindowHandle> {
        self.entries.iter().map(|entry| entry.handle).collect()
    }

    fn visibility_callbacks(&self) -> Vec<Rc<dyn Fn()>> {
        self.entries
            .iter()
            .map(|entry| entry.sync_visibility.clone())
            .collect()
    }

    fn publish_state(&self) {
        let Some(root) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.document_element())
        else {
            return;
        };
        let secondary_count = self
            .entries
            .iter()
            .filter(|entry| entry.host.is_some())
            .count();
        let _ = root.set_attribute("data-kael-window-count", &self.entries.len().to_string());
        let _ = root.set_attribute(
            "data-kael-secondary-window-count",
            &secondary_count.to_string(),
        );
        if let Some(surface_id) = self.active.and_then(|handle| {
            self.entries
                .iter()
                .find(|entry| entry.handle == handle)
                .map(|entry| entry.surface_id.as_str())
        }) {
            let _ = root.set_attribute("data-kael-active-window", surface_id);
        } else {
            let _ = root.remove_attribute("data-kael-active-window");
        }
    }
}

impl Drop for BrowserFilePicker {
    fn drop(&mut self) {
        let target: &EventTarget = self.input.unchecked_ref();
        for (name, callback) in &self.callbacks {
            let _ =
                target.remove_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
        }
        self.input.remove();
    }
}

#[derive(Default)]
pub(super) struct WebClipboardState {
    item: Option<ClipboardItem>,
    revision: u64,
}

impl WebClipboardState {
    pub(super) fn store(&mut self, item: ClipboardItem) {
        self.item = Some(item);
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn clear(&mut self) {
        self.item = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn snapshot(&self) -> Option<ClipboardItem> {
        self.item.clone()
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
}

/// The browser's single CSS-pixel display surface.
#[derive(Debug, Default)]
pub(crate) struct WebDisplay {
    refresh_rate: Mutex<RefreshRateEstimator>,
}

impl WebDisplay {
    fn record_animation_frame(&self, timestamp_ms: f64) {
        self.refresh_rate.lock().record(timestamp_ms);
    }
}

impl PlatformDisplay for WebDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(1)
    }

    fn uuid(&self) -> Result<Uuid> {
        Ok(Uuid::from_u128(0x6b61_656c_0000_0000_0000_0000_7765_6201))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let (width, height) = web_sys::window()
            .and_then(|window| {
                Some((
                    window.inner_width().ok()?.as_f64()? as f32,
                    window.inner_height().ok()?.as_f64()? as f32,
                ))
            })
            .unwrap_or((1_024.0, 768.0));
        Bounds::from_corners(point(px(0.0), px(0.0)), point(px(width), px(height)))
    }

    fn refresh_rate(&self) -> Option<f32> {
        (*self.refresh_rate.lock()).refresh_rate_hz()
    }

    fn scale_factor(&self) -> f32 {
        web_sys::window().map_or(1.0, |window| window.device_pixel_ratio() as f32)
    }
}

struct WebKeyboardLayout;

impl PlatformKeyboardLayout for WebKeyboardLayout {
    fn id(&self) -> &str {
        "browser"
    }

    fn name(&self) -> &str {
        "Browser keyboard"
    }
}

/// Browser implementation of the platform services used by `App`.
pub(crate) struct WebPlatform {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<WebTextSystem>,
    display: Rc<WebDisplay>,
    windows: Rc<RefCell<WebWindowRegistry>>,
    application_hidden: Rc<Cell<bool>>,
    clipboard: Arc<Mutex<WebClipboardState>>,
    file_picker: Rc<RefCell<Option<BrowserFilePicker>>>,
    open_urls: RefCell<Option<Box<dyn FnMut(Vec<String>)>>>,
    last_dispatched_url: RefCell<Option<String>>,
    reopen_callback: RefCell<Option<Box<dyn FnMut()>>>,
    quit_callback: RefCell<Option<Box<dyn FnMut()>>>,
    platform_listeners: RefCell<Vec<BrowserPlatformListener>>,
}

impl WebPlatform {
    pub(crate) fn new() -> Result<Rc<Self>> {
        console_error_panic_hook::set_once();
        web_sys::window().context("Kael's browser backend requires a Window")?;
        let dispatcher = WebDispatcher::new();
        let platform = Rc::new(Self {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system: Arc::new(WebTextSystem::new()),
            display: Rc::new(WebDisplay::default()),
            windows: Rc::new(RefCell::new(WebWindowRegistry::default())),
            application_hidden: Rc::new(Cell::new(false)),
            clipboard: Arc::new(Mutex::new(WebClipboardState::default())),
            file_picker: Rc::new(RefCell::new(None)),
            open_urls: RefCell::new(None),
            last_dispatched_url: RefCell::new(None),
            reopen_callback: RefCell::new(None),
            quit_callback: RefCell::new(None),
            platform_listeners: RefCell::new(Vec::new()),
        });
        platform.install_platform_listeners()?;
        Ok(platform)
    }

    fn canvas() -> Option<web_sys::HtmlCanvasElement> {
        use wasm_bindgen::JsCast as _;
        web_sys::window()?
            .document()?
            .get_element_by_id("blade")?
            .dyn_into()
            .ok()
    }

    fn add_platform_listener(
        self: &Rc<Self>,
        name: &'static str,
        mut callback: impl FnMut(&Self, Event) + 'static,
    ) -> Result<()> {
        let target: EventTarget = web_sys::window()
            .context("browser Window is unavailable")?
            .unchecked_into();
        let weak = Rc::downgrade(self);
        let closure = Closure::wrap(Box::new(move |event: Event| {
            if let Some(platform) = weak.upgrade() {
                callback(&platform, event);
            }
        }) as Box<dyn FnMut(Event)>);
        target
            .add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())
            .map_err(js_error)?;
        self.platform_listeners
            .borrow_mut()
            .push(BrowserPlatformListener {
                target,
                name,
                callback: closure,
            });
        Ok(())
    }

    fn install_platform_listeners(self: &Rc<Self>) -> Result<()> {
        for name in ["popstate", "hashchange"] {
            self.add_platform_listener(name, |platform, _| {
                platform.dispatch_current_url();
            })?;
        }
        self.add_platform_listener("pageshow", |platform, _| {
            platform.application_hidden.set(false);
            platform.sync_window_visibilities();
            platform.dispatch_current_url();
            if let Some(callback) = platform.reopen_callback.borrow_mut().as_mut() {
                callback();
            }
        })?;
        self.add_platform_listener("pagehide", |platform, _| {
            platform.application_hidden.set(true);
            // Hiding a focused canvas can synchronously dispatch `blur`, which
            // updates the active-window registry. Snapshot callbacks so no
            // registry borrow is held while browser DOM events can re-enter us.
            platform.sync_window_visibilities();
        })?;
        Ok(())
    }

    fn sync_window_visibilities(&self) {
        let visibility_callbacks = self.windows.borrow().visibility_callbacks();
        for callback in visibility_callbacks {
            callback();
        }
    }

    fn current_url() -> Option<String> {
        web_sys::window()?.location().href().ok()
    }

    fn dispatch_current_url(&self) {
        let Some(url) = Self::current_url() else {
            return;
        };
        if self.last_dispatched_url.borrow().as_deref() == Some(url.as_str()) {
            return;
        }
        if let Some(callback) = self.open_urls.borrow_mut().as_mut() {
            *self.last_dispatched_url.borrow_mut() = Some(url.clone());
            callback(vec![url]);
        }
    }
}

fn write_browser_clipboard(item: ClipboardItem) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let clipboard = window.navigator().clipboard();
    let fallback_text = item.text();
    let promise = browser_clipboard_write_promise(&clipboard, &item)
        .ok()
        .flatten()
        .or_else(|| {
            fallback_text
                .as_deref()
                .map(|text| clipboard.write_text(text))
        });
    if let Some(promise) = promise {
        // Clipboard writes require a secure context and normally a user
        // activation. Retain the in-process mirror regardless, and consume
        // rejections so a denied optional system write is not an unhandled
        // browser promise.
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

fn browser_clipboard_write_promise(
    clipboard: &web_sys::Clipboard,
    item: &ClipboardItem,
) -> Result<Option<js_sys::Promise>> {
    let record = js_sys::Object::new();
    let mut has_entry = false;

    if let Some(text) = item.text().filter(|text| !text.is_empty()) {
        set_clipboard_blob(&record, "text/plain", string_blob(&text, "text/plain")?)?;
        has_entry = true;
    }
    if let Some(html) = item.html().filter(|html| !html.is_empty()) {
        set_clipboard_blob(&record, "text/html", string_blob(&html, "text/html")?)?;
        has_entry = true;
    }
    if let Some(image) = item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image),
        ClipboardEntry::String(_) => None,
    }) {
        let mime = image.format().mime_type();
        if image.has_bytes() && BrowserClipboardItem::supports(mime) {
            set_clipboard_blob(&record, mime, bytes_blob(image.bytes(), mime)?)?;
            has_entry = true;
        }
    }

    if !has_entry {
        return Ok(None);
    }
    let browser_item = BrowserClipboardItem::new_with_record_from_str_to_blob_promise(&record)
        .map_err(js_error)?;
    let items = js_sys::Array::new();
    items.push(browser_item.as_ref());
    Ok(Some(clipboard.write(items.as_ref())))
}

fn set_clipboard_blob(record: &js_sys::Object, mime: &str, blob: Blob) -> Result<()> {
    js_sys::Reflect::set(record.as_ref(), &JsValue::from_str(mime), blob.as_ref())
        .map(|_| ())
        .map_err(js_error)
}

fn string_blob(value: &str, mime: &str) -> Result<Blob> {
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(value));
    let options = BlobPropertyBag::new();
    options.set_type(mime);
    Blob::new_with_str_sequence_and_options(parts.as_ref(), &options).map_err(js_error)
}

fn bytes_blob(bytes: &[u8], mime: &str) -> Result<Blob> {
    let parts = js_sys::Array::new();
    parts.push(js_sys::Uint8Array::from(bytes).as_ref());
    let options = BlobPropertyBag::new();
    options.set_type(mime);
    Blob::new_with_u8_array_sequence_and_options(parts.as_ref(), &options).map_err(js_error)
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

impl Platform for WebPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        on_finish_launching();
    }

    fn quit(&self) {
        self.application_hidden.set(true);
        self.sync_window_visibilities();
        if let Some(mut callback) = self.quit_callback.borrow_mut().take() {
            callback();
        }
    }

    fn restart(&self, _binary_path: Option<PathBuf>) {
        if let Some(window) = web_sys::window() {
            let _ = window.location().reload();
        }
    }

    fn activate(&self, _ignoring_other_apps: bool) {
        self.application_hidden.set(false);
        self.sync_window_visibilities();
        // `focus()` dispatches DOM focus/blur listeners synchronously. Clone the
        // target while the registry is borrowed, then release the `Ref` before
        // entering browser code so those listeners can update the registry.
        let canvas = { self.windows.borrow().active_canvas() }.or_else(Self::canvas);
        if let Some(canvas) = canvas {
            let _ = canvas.focus();
        }
    }

    fn hide(&self) {
        self.application_hidden.set(true);
        self.sync_window_visibilities();
    }

    fn hide_other_apps(&self) {}
    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![self.display.clone()]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.display.clone())
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        screen_capture::is_supported()
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>> {
        screen_capture::sources()
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.windows.borrow().active
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        Some(self.windows.borrow().handles())
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn crate::PlatformWindow>> {
        let primary = self.windows.borrow().entries.is_empty();
        let surface_id = self.windows.borrow_mut().allocate_surface_id();
        let window = WebWindow::new(
            handle,
            options,
            self.display.clone(),
            self.clipboard.clone(),
            self.windows.clone(),
            self.application_hidden.clone(),
            primary,
            surface_id,
        )?;
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        browser_appearance()
    }

    fn open_url(&self, url: &str) {
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url_and_target(url, "_blank");
        }
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        *self.open_urls.borrow_mut() = Some(callback);
        self.last_dispatched_url.borrow_mut().take();
        self.dispatch_current_url();
    }

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "custom URL schemes are unavailable in browsers"
        )))
    }

    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        tx.send(Err(anyhow!(
            "native filesystem path prompts are unavailable in browsers"
        )))
        .ok();
        rx
    }

    fn prompt_for_files(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<ExternalFile>>>> {
        let (tx, rx) = oneshot::channel();
        if self.file_picker.borrow().is_some() {
            tx.send(Err(anyhow!("a browser file picker is already open")))
                .ok();
            return rx;
        }
        if !options.files && !options.directories {
            tx.send(Err(anyhow!(
                "browser file picker must allow files or directories"
            )))
            .ok();
            return rx;
        }

        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            tx.send(Err(anyhow!("browser Document is unavailable")))
                .ok();
            return rx;
        };
        let Ok(input) = document
            .create_element("input")
            .map_err(js_error)
            .and_then(|element| {
                element
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| anyhow!("failed to create browser file input"))
            })
        else {
            tx.send(Err(anyhow!("failed to create browser file input")))
                .ok();
            return rx;
        };
        input.set_type("file");
        input.set_multiple(options.multiple || options.directories);
        if options.directories {
            let _ = input.set_attribute("webkitdirectory", "");
        }
        let accept = options
            .filters
            .iter()
            .flat_map(|filter| filter.extensions())
            .map(|extension| format!(".{}", extension.as_ref()))
            .collect::<Vec<_>>()
            .join(",");
        if !accept.is_empty() {
            input.set_accept(&accept);
        }
        let prompt = options
            .prompt
            .as_ref()
            .map(|prompt| prompt.as_ref())
            .unwrap_or("Select files");
        let _ = input.set_attribute("aria-label", prompt);
        let _ = input.style().set_property("display", "none");
        let Some(body) = document.body() else {
            tx.send(Err(anyhow!("browser Document has no body"))).ok();
            return rx;
        };
        if body.append_child(&input).is_err() {
            tx.send(Err(anyhow!("failed to attach browser file input")))
                .ok();
            return rx;
        }

        let sender = Rc::new(RefCell::new(Some(tx)));
        let picker_slot = self.file_picker.clone();
        let input_for_change = input.clone();
        let sender_for_change = sender.clone();
        let slot_for_change = picker_slot.clone();
        let change = Closure::wrap(Box::new(move |_event: Event| {
            let files = input_for_change
                .files()
                .map(|files| {
                    (0..files.length())
                        .filter_map(|index| files.get(index))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let sender = sender_for_change.clone();
            let slot = slot_for_change.clone();
            spawn_local(async move {
                let files = read_browser_files(files).await;
                if let Some(sender) = sender.borrow_mut().take() {
                    sender.send(Ok(Some(files))).ok();
                }
                slot.borrow_mut().take();
            });
        }) as Box<dyn FnMut(Event)>);
        let sender_for_cancel = sender.clone();
        let slot_for_cancel = picker_slot.clone();
        let cancel = Closure::wrap(Box::new(move |_event: Event| {
            let sender = sender_for_cancel.clone();
            let slot = slot_for_cancel.clone();
            spawn_local(async move {
                let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
                if let Some(sender) = sender.borrow_mut().take() {
                    sender.send(Ok(None)).ok();
                }
                slot.borrow_mut().take();
            });
        }) as Box<dyn FnMut(Event)>);
        let target: &EventTarget = input.unchecked_ref();
        if target
            .add_event_listener_with_callback("change", change.as_ref().unchecked_ref())
            .is_err()
        {
            if let Some(sender) = sender.borrow_mut().take() {
                sender
                    .send(Err(anyhow!(
                        "failed to install browser file-picker change listener"
                    )))
                    .ok();
            }
            input.remove();
            return rx;
        }
        if target
            .add_event_listener_with_callback("cancel", cancel.as_ref().unchecked_ref())
            .is_err()
        {
            let _ = target
                .remove_event_listener_with_callback("change", change.as_ref().unchecked_ref());
            if let Some(sender) = sender.borrow_mut().take() {
                sender
                    .send(Err(anyhow!(
                        "failed to install browser file-picker cancel listener"
                    )))
                    .ok();
            }
            input.remove();
            return rx;
        }
        *picker_slot.borrow_mut() = Some(BrowserFilePicker {
            input: input.clone(),
            callbacks: vec![("change", change), ("cancel", cancel)],
        });
        input.click();
        rx
    }

    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        tx.send(Err(anyhow!(
            "native filesystem save paths are unavailable in browsers"
        )))
        .ok();
        rx
    }

    fn save_file_bytes(
        &self,
        _directory: PathBuf,
        suggested_name: Option<String>,
        mime_type: String,
        bytes: Arc<[u8]>,
    ) -> oneshot::Receiver<Result<bool>> {
        let (tx, rx) = oneshot::channel();
        let result = (|| {
            let window = web_sys::window().context("browser Window is unavailable")?;
            let document = window
                .document()
                .context("browser Document is unavailable")?;
            let body = document.body().context("browser Document has no body")?;
            let file_name = suggested_name
                .as_deref()
                .and_then(|name| name.rsplit(['/', '\\']).next())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("download");
            let blob = bytes_blob(bytes.as_ref(), &mime_type)?;
            let object_url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
            let outcome = (|| {
                let anchor = document
                    .create_element("a")
                    .map_err(js_error)?
                    .dyn_into::<HtmlElement>()
                    .map_err(|_| anyhow!("failed to create browser download anchor"))?;
                anchor
                    .set_attribute("href", &object_url)
                    .map_err(js_error)?;
                anchor
                    .set_attribute("download", file_name)
                    .map_err(js_error)?;
                let _ = anchor.style().set_property("display", "none");
                body.append_child(&anchor).map_err(js_error)?;
                anchor.click();

                // Keep both the anchor and its Blob URL alive until the
                // browser has had an opportunity to process the download's
                // default action. Some engines cancel that action if the
                // detached anchor or URL disappears in the same task.
                let cleanup_anchor = anchor.clone();
                let cleanup_url = object_url.clone();
                let cleanup = Closure::<dyn FnMut()>::new(move || {
                    cleanup_anchor.remove();
                    let _ = web_sys::Url::revoke_object_url(&cleanup_url);
                });
                if window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        cleanup.as_ref().unchecked_ref(),
                        5_000,
                    )
                    .is_ok()
                {
                    cleanup.forget();
                } else {
                    anchor.remove();
                    let _ = web_sys::Url::revoke_object_url(&object_url);
                }
                Ok(true)
            })();
            if outcome.is_err() {
                let _ = web_sys::Url::revoke_object_url(&object_url);
            }
            outcome
        })();
        tx.send(result).ok();
        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, _path: &Path) {}
    fn open_with_system(&self, _path: &Path) {}
    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        // The callback owns the application cell. Retaining it also keeps the
        // browser application alive after `Application::run` returns to JS.
        *self.quit_callback.borrow_mut() = Some(callback);
    }
    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        *self.reopen_callback.borrow_mut() = Some(callback);
    }
    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}
    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}
    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn crate::Action)>) {}
    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}
    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn crate::Action) -> bool>) {}

    fn compositor_name(&self) -> &'static str {
        "webgl2-scene"
    }

    fn app_path(&self) -> Result<PathBuf> {
        Err(anyhow!("browser applications do not have executable paths"))
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow!("browser applications cannot launch executables"))
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        let css = match style {
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
            CursorStyle::ResizeUpLeftDownRight => "nwse-resize",
            CursorStyle::ResizeUpRightDownLeft => "nesw-resize",
            CursorStyle::ResizeColumn => "col-resize",
            CursorStyle::ResizeRow => "row-resize",
            CursorStyle::IBeamCursorForVerticalLayout => "vertical-text",
            CursorStyle::OperationNotAllowed => "not-allowed",
            CursorStyle::DragLink => "alias",
            CursorStyle::DragCopy => "copy",
            CursorStyle::ContextualMenu => "context-menu",
            CursorStyle::None => "none",
        };
        let canvas = { self.windows.borrow().active_canvas() }.or_else(Self::canvas);
        if let Some(canvas) = canvas {
            let _ = canvas.style().set_property("cursor", css);
        }
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.clipboard.lock().store(item.clone());
        write_browser_clipboard(item);
    }

    fn clear_clipboard(&self) {
        self.clipboard.lock().clear();
        if let Some(window) = web_sys::window() {
            let clipboard = window.navigator().clipboard();
            spawn_local(async move {
                let _ = JsFuture::from(clipboard.write_text("")).await;
            });
        }
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.clipboard.lock().snapshot()
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "secure credential storage is unavailable in the browser backend"
        )))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Err(anyhow!(
            "secure credential storage is unavailable in the browser backend"
        )))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "secure credential storage is unavailable in the browser backend"
        )))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(WebKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}

    fn should_reduce_motion(&self) -> bool {
        web_sys::window()
            .and_then(|window| {
                window
                    .match_media("(prefers-reduced-motion: reduce)")
                    .ok()
                    .flatten()
            })
            .is_some_and(|query| query.matches())
    }
}
