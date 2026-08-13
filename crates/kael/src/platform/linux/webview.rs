use super::super::webview_common::{
    bridge_script, create_web_context, css_script, permission_kind_from_wry,
    permission_response_to_wry, to_wry_rect, webview_command_id,
};
#[cfg(feature = "wayland")]
use super::wayland::WaylandWindowStatePtr;
#[cfg(feature = "x11")]
use super::x11::X11WindowStatePtr;
use crate::{
    AsyncWindowContext, Bounds, Pixels, Point, SharedString, WebViewCookie,
    WebViewDownloadCompleted, WebViewDownloadPolicy, WebViewNewWindowPolicy,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand,
        WebViewDocumentTitleChangedHandler, WebViewDownloadCompletedHandler,
        WebViewDownloadStartedHandler, WebViewDragDropEvent, WebViewDragDropHandler,
        WebViewDragDropPolicy, WebViewNavigationHandler, WebViewNewWindowHandler,
        WebViewPageLoadEvent, WebViewPageLoadHandler, rgba_to_webview_color,
    },
};
use anyhow::{Context as _, Result};
use gtk::prelude::*;
use parking_lot::RwLock;
#[cfg(feature = "x11")]
use raw_window_handle as rwh;
use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};
use util::ResultExt;
#[cfg(feature = "wayland")]
use wry::WebViewBuilderExtUnix;
use wry::{
    DragDropEvent as WryDragDropEvent, NewWindowResponse, PageLoadEvent, WebContext, WebView,
    WebViewBuilder, WebViewExtUnix,
};

pub(crate) struct LinuxWebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    _context: Option<WebContext>,
    current_url: SharedString,
    current_html: Option<SharedString>,
    bounds: Bounds<Pixels>,
    background_color: Option<crate::Rgba>,
    opacity: f32,
    live: Rc<RefCell<PlatformWebView>>,
    live_permission_handler: Arc<RwLock<Option<crate::webview::WebViewPermissionHandler>>>,
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

#[cfg(feature = "x11")]
pub(crate) fn sync_x11_webviews(window: &X11WindowStatePtr, webviews: &[PlatformWebView]) {
    let mut active_ids: HashSet<SharedString> = HashSet::default();
    let mut state = window.state.borrow_mut();
    let scale_factor = state.scale_factor;
    let x_window = window.x_window;

    for webview in webviews {
        let webview_id = webview.instance_id.clone();
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
    let mut active_ids: HashSet<SharedString> = HashSet::default();
    let (scale_factor, parent_origin) = {
        let state = window.state.borrow();
        (state.scale, state.bounds.origin)
    };

    for webview in webviews {
        let webview_id = webview.instance_id.clone();
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

#[cfg(feature = "x11")]
pub(crate) fn dispatch_x11_webview_command(
    window: &X11WindowStatePtr,
    command: PlatformWebViewCommand,
) -> Result<()> {
    let webview_id = webview_command_id(&command);
    let mut state = window.state.borrow_mut();
    let mut matches = state
        .webviews
        .values_mut()
        .filter(|host| host.desired.id == webview_id);
    let Some(host) = matches.next() else {
        anyhow::bail!("unknown webview: {}", webview_id);
    };
    if matches.next().is_some() {
        anyhow::bail!(
            "ambiguous webview id `{}`; WebView command ids must be unique within a window",
            webview_id
        );
    }
    host.apply_command(command)
}

#[cfg(feature = "wayland")]
pub(crate) fn dispatch_wayland_webview_command(
    window: &WaylandWindowStatePtr,
    command: PlatformWebViewCommand,
) -> Result<()> {
    let webview_id = webview_command_id(&command);
    let mut state = window.state.borrow_mut();
    let mut matches = state
        .webviews
        .values_mut()
        .filter(|host| host.desired.id == webview_id);
    let Some(host) = matches.next() else {
        anyhow::bail!("unknown webview: {}", webview_id);
    };
    if matches.next().is_some() {
        anyhow::bail!(
            "ambiguous webview id `{}`; WebView command ids must be unique within a window",
            webview_id
        );
    }
    host.apply_command(command)
}

impl LinuxWebViewHost {
    #[cfg(feature = "x11")]
    fn new(
        parent: &X11WebViewParentHandle,
        desired: PlatformWebView,
        scale_factor: f32,
    ) -> Result<Self> {
        ensure_gtk_webview_runtime()?;

        let mut context = create_web_context(&desired)?;
        let live = Rc::new(RefCell::new(desired.clone()));
        let live_permission_handler = Arc::new(RwLock::new(desired.permission_handler.clone()));
        let builder = configure_webview_builder(
            if let Some(context) = context.as_mut() {
                WebViewBuilder::new_with_web_context(context)
            } else {
                WebViewBuilder::new()
            },
            &desired,
            desired.bounds,
            live.clone(),
            live_permission_handler.clone(),
        );

        let webview = builder
            .build_as_child(parent)
            .context("building Linux X11 child webview")?;
        webview.set_visible(desired.visible).log_err();

        let mut host = Self {
            current_url: desired.url.clone(),
            current_html: desired.html.clone(),
            bounds: desired.bounds,
            background_color: desired.background_color,
            opacity: -1.0,
            live,
            live_permission_handler,
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
        let live = Rc::new(RefCell::new(desired.clone()));
        let live_permission_handler = Arc::new(RwLock::new(desired.permission_handler.clone()));
        let builder = configure_webview_builder(
            if let Some(context) = context.as_mut() {
                WebViewBuilder::new_with_web_context(context)
            } else {
                WebViewBuilder::new()
            },
            &desired,
            webview_bounds,
            live.clone(),
            live_permission_handler.clone(),
        );

        let webview = builder
            .build_gtk(&fixed)
            .context("building Linux Wayland GTK overlay webview")?;

        let mut host = Self {
            current_url: desired.url.clone(),
            current_html: desired.html.clone(),
            bounds: webview_bounds,
            background_color: desired.background_color,
            opacity: -1.0,
            live,
            live_permission_handler,
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
            || self.desired.request_headers != webview.request_headers
            || self.desired.javascript_disabled != webview.javascript_disabled
            || self.desired.devtools != webview.devtools
            || self.desired.zoom_hotkeys_enabled != webview.zoom_hotkeys_enabled
            || self.desired.media_autoplay != webview.media_autoplay
            || self.desired.focused != webview.focused
            || self.desired.clipboard_access != webview.clipboard_access
    }

    fn update_desired(
        &mut self,
        desired: PlatformWebView,
        scale_factor: f32,
        parent_origin: Option<Point<Pixels>>,
    ) {
        *self.live.borrow_mut() = desired.clone();
        *self.live_permission_handler.write() = desired.permission_handler.clone();
        self.desired = desired;
        self.apply(scale_factor, parent_origin);
    }

    fn apply(&mut self, _scale_factor: f32, parent_origin: Option<Point<Pixels>>) {
        let _ = &parent_origin;
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
            if let Some(headers) = self.desired.request_headers.clone() {
                self.webview
                    .load_url_with_headers(self.desired.url.as_ref(), headers)
                    .log_err();
            } else {
                self.webview.load_url(self.desired.url.as_ref()).log_err();
            }
            self.current_url = self.desired.url.clone();
            self.current_html = None;
        } else if self.desired.url.is_empty() && self.current_html != self.desired.html {
            if let Some(html) = self.desired.html.as_ref() {
                self.webview.load_html(html.as_ref()).log_err();
            }
            self.current_html = self.desired.html.clone();
        }

        if self.background_color != self.desired.background_color {
            let color = self
                .desired
                .background_color
                .map(rgba_to_webview_color)
                .unwrap_or((255, 255, 255, 255));
            self.webview.set_background_color(color).log_err();
            self.background_color = self.desired.background_color;
        }

        if self.opacity != self.desired.opacity {
            let opacity = self.desired.opacity.clamp(0.0, 1.0) as f64;
            match &self.backend {
                LinuxWebViewBackend::X11 => self.webview.webview().set_opacity(opacity),
                #[cfg(feature = "wayland")]
                LinuxWebViewBackend::Wayland { overlay_window } => {
                    overlay_window.set_opacity(opacity)
                }
            }
            self.opacity = self.desired.opacity;
        }

        self.webview.set_visible(self.desired.visible).log_err();
    }

    fn apply_command(&mut self, command: PlatformWebViewCommand) -> Result<()> {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.webview
                    .load_url(url.as_ref())
                    .context("navigating Linux WebView")?;
                self.current_url = url;
                self.current_html = None;
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                self.webview
                    .load_url_with_headers(url.as_ref(), headers)
                    .context("navigating Linux WebView with headers")?;
                self.current_url = url;
                self.current_html = None;
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.webview
                    .load_html(html.as_ref())
                    .context("loading HTML into Linux WebView")?;
                self.current_url = SharedString::default();
                self.current_html = Some(html);
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.webview
                    .evaluate_script(script.as_ref())
                    .context("evaluating JavaScript in Linux WebView")?;
            }
            PlatformWebViewCommand::EvaluateJavaScriptWithResult {
                script, callback, ..
            } => {
                let callback_for_result = callback.clone();
                if let Err(error) = self
                    .webview
                    .evaluate_script_with_callback(script.as_ref(), move |result| {
                        callback_for_result(Ok(result.into()))
                    })
                {
                    let message: SharedString = error.to_string().into();
                    callback(Err(message.clone()));
                    anyhow::bail!(message);
                }
            }
            PlatformWebViewCommand::PostMessage { message, .. } => {
                let payload =
                    serde_json::to_string(&message).context("serializing Linux WebView message")?;
                let script = format!(
                    "(() => {{ const payload = {payload}; if (window.dispatchEvent) {{ window.dispatchEvent(new MessageEvent('message', {{ data: payload }})); }} if (typeof window.onmessage === 'function') {{ window.onmessage({{ data: payload }}); }} }})();"
                );
                self.webview
                    .evaluate_script(&script)
                    .context("posting message into Linux WebView")?;
            }
            PlatformWebViewCommand::Reload { .. } => {
                self.webview.reload().context("reloading Linux WebView")?;
            }
            PlatformWebViewCommand::GoBack { .. } => {
                self.webview
                    .evaluate_script("history.back()")
                    .context("navigating Linux WebView backward")?;
            }
            PlatformWebViewCommand::GoForward { .. } => {
                self.webview
                    .evaluate_script("history.forward()")
                    .context("navigating Linux WebView forward")?;
            }
            PlatformWebViewCommand::OpenDevTools { .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                self.webview.open_devtools();
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                anyhow::bail!("Linux WebView devtools require a debug build or `devtools` feature");
            }
            PlatformWebViewCommand::CloseDevTools { .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                self.webview.close_devtools();
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                anyhow::bail!("Linux WebView devtools require a debug build or `devtools` feature");
            }
            PlatformWebViewCommand::IsDevToolsOpen { callback, .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                callback(Ok(self.webview.is_devtools_open()));
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                callback(Err(
                    "WebView devtools state requires a debug build or backend devtools support"
                        .into(),
                ));
            }
            PlatformWebViewCommand::Print { .. } => {
                self.webview.print().context("printing Linux WebView")?;
            }
            PlatformWebViewCommand::SetZoomFactor { factor, .. } => {
                self.webview
                    .zoom(factor)
                    .context("setting Linux WebView zoom factor")?;
            }
            PlatformWebViewCommand::Focus { .. } => {
                self.webview.focus().context("focusing Linux WebView")?;
            }
            PlatformWebViewCommand::FocusParent { .. } => {
                self.webview
                    .focus_parent()
                    .context("focusing Linux WebView parent")?;
            }
            PlatformWebViewCommand::ClearBrowsingData { .. } => {
                self.webview
                    .clear_all_browsing_data()
                    .context("clearing Linux WebView browsing data")?;
            }
            PlatformWebViewCommand::ReadUrl { callback, .. } => {
                callback(
                    self.webview
                        .url()
                        .map(SharedString::from)
                        .map_err(|error| error.to_string().into()),
                );
            }
            PlatformWebViewCommand::ReadCookies { url, callback, .. } => {
                callback(read_webview_cookies(&self.webview, url));
            }
            PlatformWebViewCommand::SetCookie {
                cookie, callback, ..
            } => {
                callback(set_webview_cookie(&self.webview, cookie));
            }
            PlatformWebViewCommand::DeleteCookie {
                cookie, callback, ..
            } => {
                callback(delete_webview_cookie(&self.webview, cookie));
            }
        }
        Ok(())
    }
}

fn set_webview_cookie(webview: &WebView, cookie: WebViewCookie) -> Result<(), SharedString> {
    let cookie = webview_cookie_to_wry(cookie);
    webview
        .set_cookie(&cookie)
        .map_err(|error| error.to_string().into())
}

fn delete_webview_cookie(webview: &WebView, cookie: WebViewCookie) -> Result<(), SharedString> {
    let cookie = webview_cookie_to_wry(cookie);
    webview
        .delete_cookie(&cookie)
        .map_err(|error| error.to_string().into())
}

fn read_webview_cookies(
    webview: &WebView,
    url: Option<SharedString>,
) -> Result<Vec<WebViewCookie>, SharedString> {
    let cookies = if let Some(url) = url {
        webview.cookies_for_url(url.as_ref())
    } else {
        webview.cookies()
    }
    .map_err(|error| SharedString::from(error.to_string()))?;

    Ok(cookies.into_iter().map(webview_cookie_from_wry).collect())
}

fn webview_cookie_from_wry(cookie: wry::cookie::Cookie<'static>) -> WebViewCookie {
    WebViewCookie {
        name: cookie.name().to_string().into(),
        value: cookie.value().to_string().into(),
        domain: cookie.domain().map(|domain| domain.to_string().into()),
        path: cookie.path().map(|path| path.to_string().into()),
        secure: cookie.secure().unwrap_or(false),
        http_only: cookie.http_only().unwrap_or(false),
    }
}

fn webview_cookie_to_wry(cookie: WebViewCookie) -> wry::cookie::Cookie<'static> {
    let mut builder =
        wry::cookie::CookieBuilder::new(cookie.name.to_string(), cookie.value.to_string())
            .secure(cookie.secure)
            .http_only(cookie.http_only);

    if let Some(domain) = cookie.domain {
        builder = builder.domain(domain.to_string());
    }
    if let Some(path) = cookie.path {
        builder = builder.path(path.to_string());
    }

    builder.build()
}

fn ensure_gtk_webview_runtime() -> Result<()> {
    gtk::init().context("initializing GTK for Linux X11 webviews")
}

fn configure_webview_builder<'a>(
    mut builder: WebViewBuilder<'a>,
    desired: &PlatformWebView,
    bounds: Bounds<Pixels>,
    live: Rc<RefCell<PlatformWebView>>,
    live_permission_handler: Arc<RwLock<Option<crate::webview::WebViewPermissionHandler>>>,
) -> WebViewBuilder<'a> {
    if desired.storage_key.is_none() {
        builder = builder.with_incognito(true);
    }

    builder = builder.with_permission_handler(move |kind| {
        let handler = live_permission_handler.read().clone();
        handler
            .map(|handler| {
                let decision = super::catch_platform_callback(
                    "webview native permission policy",
                    crate::WebViewPermissionDecision::Deny,
                    || handler(permission_kind_from_wry(kind)),
                );
                permission_response_to_wry(decision)
            })
            .unwrap_or(wry::PermissionResponse::Default)
    });

    builder = builder.with_bounds(to_wry_rect(bounds));
    if let Some(color) = desired.background_color {
        builder = builder.with_background_color(rgba_to_webview_color(color));
    }

    if !desired.url.is_empty()
        && let Some(headers) = desired.request_headers.clone()
    {
        builder = builder.with_url_and_headers(desired.url.as_ref(), headers);
    } else if !desired.url.is_empty() {
        builder = builder.with_url(desired.url.as_ref());
    } else if let Some(html) = desired.html.as_ref() {
        builder = builder.with_html(html.as_ref());
    }

    if let Some(user_agent) = &desired.user_agent {
        builder = builder.with_user_agent(user_agent.as_ref());
    }
    builder = builder.with_devtools(desired.devtools);
    builder = builder.with_hotkeys_zoom(desired.zoom_hotkeys_enabled);
    if let Some(autoplay) = desired.media_autoplay {
        builder = builder.with_autoplay(autoplay);
    }
    if let Some(focused) = desired.focused {
        builder = builder.with_focused(focused);
    }
    if desired.javascript_disabled {
        builder = builder.with_javascript_disabled();
    }
    builder = builder.with_clipboard(desired.clipboard_access);

    let ipc_live = live.clone();
    builder = builder.with_ipc_handler(move |request| {
        let (handler, mut async_window) = {
            let live = ipc_live.borrow();
            (live.message_handler.clone(), live.async_window.clone())
        };
        let Some(handler) = handler else {
            return;
        };

        let body = request.body().to_string();
        let payload =
            serde_json::from_str(&body).unwrap_or_else(|_| serde_json::Value::String(body));
        super::catch_platform_callback("webview message", (), || {
            let _ = async_window.update(|window, cx| handler(payload, window, cx));
        });
    });

    let navigation_live = live.clone();
    builder = builder.with_navigation_handler(move |url| {
        let (handler, mut async_window) = {
            let live = navigation_live.borrow();
            (live.navigation_handler.clone(), live.async_window.clone())
        };
        if let Some(handler) = handler {
            super::catch_platform_callback(
                "webview navigation policy",
                NavigationPolicy::Deny,
                || {
                    async_window
                        .update(|window, cx| handler(url.clone().into(), window, cx))
                        .unwrap_or(NavigationPolicy::Deny)
                },
            ) == NavigationPolicy::Allow
        } else {
            true
        }
    });

    let new_window_live = live.clone();
    builder = builder.with_new_window_req_handler(move |url, _features| {
        let (new_window_handler, navigation_handler, async_window, webview_id) = {
            let live = new_window_live.borrow();
            (
                live.new_window_handler.clone(),
                live.navigation_handler.clone(),
                live.async_window.clone(),
                live.id.clone(),
            )
        };
        match resolve_new_window_policy(
            &url,
            new_window_handler,
            navigation_handler,
            async_window.clone(),
        ) {
            WebViewNewWindowPolicy::Deny => {}
            WebViewNewWindowPolicy::NavigateCurrent => {
                let mut async_window = async_window;
                let _ = async_window.update(|window, _| {
                    let _ = window.navigate_webview(webview_id, url.clone());
                });
            }
            WebViewNewWindowPolicy::Allow => return NewWindowResponse::Allow,
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

    let download_started_live = live.clone();
    builder = builder.with_download_started_handler(move |url, path| {
        let (handler, async_window) = {
            let live = download_started_live.borrow();
            (
                live.download_started_handler.clone(),
                live.async_window.clone(),
            )
        };
        resolve_download_started(url, path, handler, async_window)
    });

    let download_completed_live = live.clone();
    builder = builder.with_download_completed_handler(move |url, path, success| {
        let (handler, async_window) = {
            let live = download_completed_live.borrow();
            (
                live.download_completed_handler.clone(),
                live.async_window.clone(),
            )
        };
        if let Some(handler) = handler {
            dispatch_download_completed(url, path, success, handler, async_window);
        }
    });

    let title_live = live.clone();
    builder = builder.with_document_title_changed_handler(move |title| {
        let (handler, async_window) = {
            let live = title_live.borrow();
            (
                live.document_title_changed_handler.clone(),
                live.async_window.clone(),
            )
        };
        if let Some(handler) = handler {
            dispatch_document_title_changed(title, handler, async_window);
        }
    });

    let page_load_live = live.clone();
    builder = builder.with_on_page_load_handler(move |event, url| {
        let (handler, async_window) = {
            let live = page_load_live.borrow();
            (live.page_load_handler.clone(), live.async_window.clone())
        };
        if let Some(handler) = handler {
            dispatch_page_load(event, url, handler, async_window);
        }
    });

    builder = builder.with_drag_drop_handler(move |event| {
        let (handler, async_window) = {
            let live = live.borrow();
            (live.drag_drop_handler.clone(), live.async_window.clone())
        };
        handler.is_some_and(|handler| {
            dispatch_drag_drop_event(event, handler, async_window).blocks_browser_default()
        })
    });

    builder
}

fn dispatch_drag_drop_event(
    event: WryDragDropEvent,
    handler: WebViewDragDropHandler,
    async_window: AsyncWindowContext,
) -> WebViewDragDropPolicy {
    let event = match event {
        WryDragDropEvent::Enter { paths, position } => {
            WebViewDragDropEvent::Enter { paths, position }
        }
        WryDragDropEvent::Over { position } => WebViewDragDropEvent::Over { position },
        WryDragDropEvent::Drop { paths, position } => {
            WebViewDragDropEvent::Drop { paths, position }
        }
        WryDragDropEvent::Leave => WebViewDragDropEvent::Leave,
        _ => WebViewDragDropEvent::Leave,
    };
    let mut async_window = async_window.clone();
    super::catch_platform_callback(
        "webview drag-and-drop",
        WebViewDragDropPolicy::BlockBrowserDefault,
        || {
            async_window
                .update(|window, cx| handler(event, window, cx))
                .unwrap_or(WebViewDragDropPolicy::BlockBrowserDefault)
        },
    )
}

fn dispatch_page_load(
    event: PageLoadEvent,
    url: String,
    handler: WebViewPageLoadHandler,
    async_window: AsyncWindowContext,
) {
    let event = match event {
        PageLoadEvent::Started => WebViewPageLoadEvent::Started,
        PageLoadEvent::Finished => WebViewPageLoadEvent::Finished,
    };
    let mut async_window = async_window.clone();
    super::catch_platform_callback("webview page load", (), || {
        let _ = async_window.update(|window, cx| handler(event, url.into(), window, cx));
    });
}

fn dispatch_document_title_changed(
    title: String,
    handler: WebViewDocumentTitleChangedHandler,
    async_window: AsyncWindowContext,
) {
    let mut async_window = async_window.clone();
    super::catch_platform_callback("webview title change", (), || {
        let _ = async_window.update(|window, cx| handler(title.into(), window, cx));
    });
}

fn resolve_download_started(
    url: String,
    path: &mut std::path::PathBuf,
    handler: Option<WebViewDownloadStartedHandler>,
    async_window: AsyncWindowContext,
) -> bool {
    let Some(handler) = handler else {
        return true;
    };

    let suggested_path = if path.as_os_str().is_empty() {
        None
    } else {
        Some(path.clone())
    };
    let mut async_window = async_window.clone();
    match super::catch_platform_callback(
        "webview download started",
        WebViewDownloadPolicy::Deny,
        || {
            async_window
                .update(|window, cx| handler(url.into(), suggested_path, window, cx))
                .unwrap_or(WebViewDownloadPolicy::Deny)
        },
    ) {
        WebViewDownloadPolicy::Allow => true,
        WebViewDownloadPolicy::Deny => false,
        WebViewDownloadPolicy::SaveTo(destination) => {
            if destination.is_absolute() {
                *path = destination;
                true
            } else {
                log::warn!(
                    "WebView download destination must be absolute: {}",
                    destination.display()
                );
                false
            }
        }
    }
}

fn dispatch_download_completed(
    url: String,
    path: Option<std::path::PathBuf>,
    success: bool,
    handler: WebViewDownloadCompletedHandler,
    async_window: AsyncWindowContext,
) {
    let event = WebViewDownloadCompleted {
        url: url.into(),
        path,
        success,
    };
    let mut async_window = async_window.clone();
    super::catch_platform_callback("webview download completed", (), || {
        let _ = async_window.update(|window, cx| handler(event, window, cx));
    });
}

fn resolve_new_window_policy(
    url: &str,
    new_window_handler: Option<WebViewNewWindowHandler>,
    navigation_handler: Option<WebViewNavigationHandler>,
    async_window: AsyncWindowContext,
) -> WebViewNewWindowPolicy {
    if let Some(handler) = new_window_handler {
        let mut async_window = async_window.clone();
        return super::catch_platform_callback(
            "webview new-window policy",
            WebViewNewWindowPolicy::Deny,
            || {
                async_window
                    .update(|window, cx| handler(url.to_string().into(), window, cx))
                    .unwrap_or(WebViewNewWindowPolicy::Deny)
            },
        );
    }

    if let Some(navigation_handler) = navigation_handler {
        let mut async_window = async_window.clone();
        return if super::catch_platform_callback(
            "webview navigation policy",
            NavigationPolicy::Deny,
            || {
                async_window
                    .update(|window, cx| navigation_handler(url.to_string().into(), window, cx))
                    .unwrap_or(NavigationPolicy::Deny)
            },
        ) == NavigationPolicy::Allow
        {
            WebViewNewWindowPolicy::NavigateCurrent
        } else {
            WebViewNewWindowPolicy::Deny
        };
    }

    WebViewNewWindowPolicy::NavigateCurrent
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

#[cfg(feature = "x11")]
struct X11WebViewParentHandle {
    window_id: u32,
}

#[cfg(feature = "x11")]
impl rwh::HasWindowHandle for X11WebViewParentHandle {
    fn window_handle(&self) -> std::result::Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let handle = rwh::XlibWindowHandle::new(self.window_id as std::ffi::c_ulong);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(handle.into()) })
    }
}
