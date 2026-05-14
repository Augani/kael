#[cfg(feature = "wayland")]
use super::wayland::WaylandWindowStatePtr;
use super::x11::X11WindowStatePtr;
use crate::{
    Bounds, Pixels, Point, SharedString,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand, WebViewMessageHandler,
        WebViewNavigationHandler,
    },
};
use anyhow::{Context as _, Result};
use gtk::prelude::*;
use raw_window_handle as rwh;
use std::{collections::HashSet, env, fs, path::PathBuf, rc::Rc};
use util::ResultExt;
#[cfg(feature = "wayland")]
use wry::WebViewBuilderExtUnix;
use wry::{
    NewWindowResponse, Rect, WebContext, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

pub(crate) struct LinuxWebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    _context: Option<WebContext>,
    current_url: SharedString,
    bounds: Bounds<Pixels>,
    backend: LinuxWebViewBackend,
}

enum LinuxWebViewBackend {
    X11,
    #[cfg(feature = "wayland")]
    Wayland {
        overlay_window: gtk::Window,
    },
}

impl Drop for LinuxWebViewHost {
    fn drop(&mut self) {
        #[cfg(feature = "wayland")]
        if let LinuxWebViewBackend::Wayland { overlay_window } = &self.backend {
            overlay_window.close();
        }
    }
}

pub(crate) fn pump_gtk_webview_events() {
    if !gtk::is_initialized_main_thread() {
        return;
    }

    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
}

pub(crate) fn sync_x11_webviews(window: &X11WindowStatePtr, webviews: &[PlatformWebView]) {
    let mut active_ids = HashSet::default();
    let mut state = window.state.borrow_mut();
    let scale_factor = state.scale_factor;
    let x_window = window.x_window;

    for webview in webviews {
        let webview_id = webview.id.clone();
        active_ids.insert(webview_id.clone());

        let needs_recreate = state
            .webviews
            .get(&webview_id)
            .is_some_and(|host| host.needs_recreate(webview));
        if needs_recreate {
            state.webviews.remove(&webview_id);
        }

        if let Some(host) = state.webviews.get_mut(&webview_id) {
            host.update_desired(webview.clone(), scale_factor, None);
        } else {
            let parent = X11WebViewParentHandle {
                window_id: x_window,
            };
            match LinuxWebViewHost::new(&parent, webview.clone(), scale_factor) {
                Ok(host) => {
                    state.webviews.insert(webview_id, host);
                }
                Err(error) => {
                    log::error!(
                        "failed to create Linux X11 WebView {}: {error:#}",
                        webview.id
                    );
                }
            }
        }
    }

    let stale_ids = state
        .webviews
        .keys()
        .filter(|webview_id| !active_ids.contains(*webview_id))
        .cloned()
        .collect::<Vec<_>>();
    for webview_id in stale_ids {
        state.webviews.remove(&webview_id);
    }
}

#[cfg(feature = "wayland")]
pub(crate) fn sync_wayland_webviews(window: &WaylandWindowStatePtr, webviews: &[PlatformWebView]) {
    let mut active_ids = HashSet::default();
    let (scale_factor, parent_origin) = {
        let state = window.state.borrow();
        (state.scale, state.bounds.origin)
    };

    for webview in webviews {
        let webview_id = webview.id.clone();
        active_ids.insert(webview_id.clone());

        let needs_recreate = {
            let state = window.state.borrow();
            state
                .webviews
                .get(&webview_id)
                .is_some_and(|host| host.needs_recreate(webview))
        };

        if needs_recreate {
            window.state.borrow_mut().webviews.remove(&webview_id);
        }

        let updated_existing = {
            let mut state = window.state.borrow_mut();
            if let Some(host) = state.webviews.get_mut(&webview_id) {
                host.update_desired(webview.clone(), scale_factor, Some(parent_origin));
                true
            } else {
                false
            }
        };

        if updated_existing {
            continue;
        }

        match LinuxWebViewHost::new_wayland(webview.clone(), scale_factor, parent_origin) {
            Ok(host) => {
                window.state.borrow_mut().webviews.insert(webview_id, host);
            }
            Err(error) => {
                log::error!(
                    "failed to create Linux Wayland WebView {}: {error:#}",
                    webview.id
                );
            }
        }
    }

    let stale_ids = {
        let state = window.state.borrow();
        state
            .webviews
            .keys()
            .filter(|webview_id| !active_ids.contains(*webview_id))
            .cloned()
            .collect::<Vec<_>>()
    };
    for webview_id in stale_ids {
        window.state.borrow_mut().webviews.remove(&webview_id);
    }
}

pub(crate) fn dispatch_x11_webview_command(
    window: &X11WindowStatePtr,
    command: PlatformWebViewCommand,
) -> Result<()> {
    let webview_id = webview_command_id(&command);
    let mut state = window.state.borrow_mut();
    let Some(host) = state.webviews.get_mut(&webview_id) else {
        anyhow::bail!("unknown webview: {}", webview_id);
    };
    host.apply_command(command);
    Ok(())
}

#[cfg(feature = "wayland")]
pub(crate) fn dispatch_wayland_webview_command(
    window: &WaylandWindowStatePtr,
    command: PlatformWebViewCommand,
) -> Result<()> {
    let webview_id = webview_command_id(&command);
    let mut state = window.state.borrow_mut();
    let Some(host) = state.webviews.get_mut(&webview_id) else {
        anyhow::bail!("unknown webview: {}", webview_id);
    };
    host.apply_command(command);
    Ok(())
}

impl LinuxWebViewHost {
    fn new(
        parent: &X11WebViewParentHandle,
        desired: PlatformWebView,
        scale_factor: f32,
    ) -> Result<Self> {
        ensure_gtk_webview_runtime()?;

        let mut context = create_web_context(&desired)?;
        let builder = configure_webview_builder(
            if let Some(context) = context.as_mut() {
                WebViewBuilder::new_with_web_context(context)
            } else {
                WebViewBuilder::new()
            },
            &desired,
            desired.bounds,
        );

        let webview = builder
            .build_as_child(parent)
            .context("building Linux X11 child webview")?;
        webview.set_visible(desired.visible).log_err();

        let mut host = Self {
            current_url: desired.url.clone(),
            bounds: desired.bounds,
            desired,
            webview,
            _context: context,
            backend: LinuxWebViewBackend::X11,
        };
        host.apply(scale_factor, None);
        Ok(host)
    }

    #[cfg(feature = "wayland")]
    fn new_wayland(
        desired: PlatformWebView,
        scale_factor: f32,
        parent_origin: Point<Pixels>,
    ) -> Result<Self> {
        ensure_gtk_webview_runtime()?;

        let overlay_window = gtk::Window::new(gtk::WindowType::Toplevel);
        overlay_window.set_decorated(false);
        overlay_window.set_resizable(false);
        overlay_window.set_skip_taskbar_hint(true);
        overlay_window.set_skip_pager_hint(true);
        overlay_window.set_keep_above(true);
        overlay_window.set_accept_focus(true);
        overlay_window.set_focus_on_map(false);
        overlay_window.set_type_hint(gtk::gdk::WindowTypeHint::Utility);

        let fixed = gtk::Fixed::new();
        fixed.show();
        overlay_window.add(&fixed);

        let webview_bounds = zero_origin_bounds(desired.bounds);
        let mut context = create_web_context(&desired)?;
        let builder = configure_webview_builder(
            if let Some(context) = context.as_mut() {
                WebViewBuilder::new_with_web_context(context)
            } else {
                WebViewBuilder::new()
            },
            &desired,
            webview_bounds,
        );

        let webview = builder
            .build_gtk(&fixed)
            .context("building Linux Wayland GTK overlay webview")?;

        let mut host = Self {
            current_url: desired.url.clone(),
            bounds: webview_bounds,
            desired,
            webview,
            _context: context,
            backend: LinuxWebViewBackend::Wayland { overlay_window },
        };
        host.apply(scale_factor, Some(parent_origin));
        Ok(host)
    }

    fn needs_recreate(&self, webview: &PlatformWebView) -> bool {
        self.desired.storage_key != webview.storage_key
            || self.desired.user_agent != webview.user_agent
            || self.desired.injected_css != webview.injected_css
            || self.desired.injected_javascript != webview.injected_javascript
            || !same_optional_message_handler(
                &self.desired.message_handler,
                &webview.message_handler,
            )
            || !same_optional_navigation_handler(
                &self.desired.navigation_handler,
                &webview.navigation_handler,
            )
    }

    fn update_desired(
        &mut self,
        desired: PlatformWebView,
        scale_factor: f32,
        parent_origin: Option<Point<Pixels>>,
    ) {
        self.desired = desired;
        self.apply(scale_factor, parent_origin);
    }

    fn apply(&mut self, _scale_factor: f32, parent_origin: Option<Point<Pixels>>) {
        match &self.backend {
            LinuxWebViewBackend::X11 => {
                if self.bounds != self.desired.bounds {
                    self.webview
                        .set_bounds(to_wry_rect(self.desired.bounds))
                        .log_err();
                    self.bounds = self.desired.bounds;
                }
            }
            #[cfg(feature = "wayland")]
            LinuxWebViewBackend::Wayland { overlay_window } => {
                let overlay_bounds = parent_origin
                    .map(|origin| overlay_bounds(origin, self.desired.bounds))
                    .unwrap_or(self.desired.bounds);
                let webview_bounds = zero_origin_bounds(self.desired.bounds);

                overlay_window.move_(
                    overlay_bounds.origin.x.0 as i32,
                    overlay_bounds.origin.y.0 as i32,
                );
                overlay_window.resize(
                    overlay_bounds.size.width.0.max(1.0) as i32,
                    overlay_bounds.size.height.0.max(1.0) as i32,
                );

                if self.bounds != webview_bounds {
                    self.webview
                        .set_bounds(to_wry_rect(webview_bounds))
                        .log_err();
                    self.bounds = webview_bounds;
                }

                if self.desired.visible {
                    overlay_window.show_all();
                } else {
                    overlay_window.hide();
                }
            }
        }

        if !self.desired.url.is_empty() && self.current_url != self.desired.url {
            self.webview.load_url(self.desired.url.as_ref()).log_err();
            self.current_url = self.desired.url.clone();
        }

        self.webview.set_visible(self.desired.visible).log_err();
    }

    fn apply_command(&mut self, command: PlatformWebViewCommand) {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.webview.load_url(url.as_ref()).log_err();
                self.current_url = url;
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.webview.evaluate_script(script.as_ref()).log_err();
            }
            PlatformWebViewCommand::PostMessage { message, .. } => {
                let payload = serde_json::to_string(&message).unwrap_or_else(|_| "null".into());
                let script = format!(
                    "(() => {{ const payload = {payload}; if (window.dispatchEvent) {{ window.dispatchEvent(new MessageEvent('message', {{ data: payload }})); }} if (typeof window.onmessage === 'function') {{ window.onmessage({{ data: payload }}); }} }})();"
                );
                self.webview.evaluate_script(&script).log_err();
            }
            PlatformWebViewCommand::Reload { .. } => {
                self.webview.reload().log_err();
            }
            PlatformWebViewCommand::GoBack { .. } => {
                self.webview.evaluate_script("history.back()").log_err();
            }
            PlatformWebViewCommand::GoForward { .. } => {
                self.webview.evaluate_script("history.forward()").log_err();
            }
        }
    }
}

fn ensure_gtk_webview_runtime() -> Result<()> {
    gtk::init().context("initializing GTK for Linux X11 webviews")
}

fn create_web_context(desired: &PlatformWebView) -> Result<Option<WebContext>> {
    desired
        .storage_key
        .as_ref()
        .map(webview_storage_dir)
        .transpose()
        .map(|directory| directory.map(|data_directory| WebContext::new(Some(data_directory))))
}

fn configure_webview_builder<'a>(
    mut builder: WebViewBuilder<'a>,
    desired: &PlatformWebView,
    bounds: Bounds<Pixels>,
) -> WebViewBuilder<'a> {
    builder = builder.with_bounds(to_wry_rect(bounds));

    if !desired.url.is_empty() {
        builder = builder.with_url(desired.url.as_ref());
    }

    if let Some(user_agent) = &desired.user_agent {
        builder = builder.with_user_agent(user_agent.as_ref());
    }

    let message_handler = desired.message_handler.clone();
    let ipc_async_window = desired.async_window.clone();
    builder = builder.with_ipc_handler(move |request| {
        let Some(handler) = message_handler.clone() else {
            return;
        };

        let body = request.body().to_string();
        let payload =
            serde_json::from_str(&body).unwrap_or_else(|_| serde_json::Value::String(body));
        let mut async_window = ipc_async_window.clone();
        let _ = async_window.update(|window, cx| handler(payload, window, cx));
    });

    if let Some(navigation_handler) = desired.navigation_handler.clone() {
        let navigation_async_window = desired.async_window.clone();
        builder = builder.with_navigation_handler(move |url| {
            let mut async_window = navigation_async_window.clone();
            async_window
                .update(|window, cx| navigation_handler(url.clone().into(), window, cx))
                .unwrap_or(NavigationPolicy::Deny)
                == NavigationPolicy::Allow
        });
    }

    let new_window_async_window = desired.async_window.clone();
    let new_window_id = desired.id.clone();
    let new_window_navigation_handler = desired.navigation_handler.clone();
    builder = builder.with_new_window_req_handler(move |url, _features| {
        let allow = if let Some(navigation_handler) = new_window_navigation_handler.clone() {
            let mut async_window = new_window_async_window.clone();
            async_window
                .update(|window, cx| navigation_handler(url.clone().into(), window, cx))
                .unwrap_or(NavigationPolicy::Deny)
                == NavigationPolicy::Allow
        } else {
            true
        };

        if allow {
            let mut async_window = new_window_async_window.clone();
            let webview_id = new_window_id.clone();
            let _ = async_window.update(|window, _| {
                let _ = window.navigate_webview(webview_id, url.clone());
            });
        }

        NewWindowResponse::Deny
    });

    builder = builder.with_initialization_script(bridge_script(desired.storage_key.as_ref()));
    for css in &desired.injected_css {
        builder = builder.with_initialization_script(css_script(css.as_ref()));
    }
    for javascript in &desired.injected_javascript {
        builder = builder.with_initialization_script(javascript.as_ref());
    }

    builder
}

fn webview_storage_dir(storage_key: &SharedString) -> Result<PathBuf> {
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .context("XDG_DATA_HOME or HOME environment variable not set for webview storage")?;

    let directory = base
        .join(env!("CARGO_PKG_NAME"))
        .join("webview")
        .join(format!(
            "{:016x}",
            seahash::hash(storage_key.as_ref().as_bytes())
        ));
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "creating Linux webview storage directory {}",
            directory.display()
        )
    })?;
    Ok(directory)
}

fn webview_command_id(command: &PlatformWebViewCommand) -> SharedString {
    match command {
        PlatformWebViewCommand::Navigate { id, .. }
        | PlatformWebViewCommand::EvaluateJavaScript { id, .. }
        | PlatformWebViewCommand::PostMessage { id, .. }
        | PlatformWebViewCommand::Reload { id }
        | PlatformWebViewCommand::GoBack { id }
        | PlatformWebViewCommand::GoForward { id } => id.clone(),
    }
}

fn to_wry_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        position: LogicalPosition::new(bounds.origin.x.0 as f64, bounds.origin.y.0 as f64).into(),
        size: LogicalSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64).into(),
    }
}

#[cfg(feature = "wayland")]
fn zero_origin_bounds(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(Point::default(), bounds.size)
}

#[cfg(feature = "wayland")]
fn overlay_bounds(parent_origin: Point<Pixels>, child_bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let mut origin = parent_origin;
    origin.x += child_bounds.origin.x;
    origin.y += child_bounds.origin.y;
    Bounds::new(origin, child_bounds.size)
}

fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn bridge_script(storage_key: Option<&SharedString>) -> String {
    let storage_key = storage_key
        .map(|storage_key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string_literal(storage_key.as_ref())
            )
        })
        .unwrap_or_default();

    format!(
        "(() => {{ {storage_key} if (!window.external) {{ window.external = {{}}; }} window.external.invoke = function(message) {{ const payload = typeof message === 'string' ? message : JSON.stringify(message); window.ipc.postMessage(payload); }}; if (!window.gpui) {{ window.gpui = {{}}; }} window.gpui.postMessage = function(message) {{ window.external.invoke(message); }}; }})();"
    )
}

fn css_script(css: &str) -> String {
    format!(
        "(() => {{ const mount = () => {{ if (!document.head) {{ return; }} const style = document.createElement('style'); style.setAttribute('data-gpui-webview-style', 'true'); style.textContent = {}; document.head.appendChild(style); }}; if (document.head) {{ mount(); }} else {{ document.addEventListener('DOMContentLoaded', mount, {{ once: true }}); }} }})();",
        json_string_literal(css)
    )
}

fn same_optional_message_handler(
    left: &Option<WebViewMessageHandler>,
    right: &Option<WebViewMessageHandler>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn same_optional_navigation_handler(
    left: &Option<WebViewNavigationHandler>,
    right: &Option<WebViewNavigationHandler>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

struct X11WebViewParentHandle {
    window_id: u32,
}

impl rwh::HasWindowHandle for X11WebViewParentHandle {
    fn window_handle(&self) -> std::result::Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let handle = rwh::XlibWindowHandle::new(self.window_id as std::ffi::c_ulong);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(handle.into()) })
    }
}
