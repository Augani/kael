use super::super::webview_common::{
    WryCustomProtocolRegistration, bridge_script, configure_wry_custom_protocols,
    create_web_context, css_script, decode_bridge_message, ipc_source_matches_top_level,
    main_frame_script, permission_kind_from_wry, permission_response_to_wry, serialized_origin,
    to_wry_rect, warn_rejected_ipc_once, webview_command_id,
};
#[cfg(feature = "wayland")]
use super::wayland::WaylandWindowStatePtr;
#[cfg(feature = "x11")]
use super::x11::X11WindowStatePtr;
use crate::{
    AsyncWindowContext, Bounds, Pixels, SharedString, WebViewCookie, WebViewDownloadCompleted,
    WebViewDownloadPolicy, WebViewNewWindowPolicy,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand, WebViewCookieCallback,
        WebViewCookieMutationCallback, WebViewDocumentTitleChangedHandler,
        WebViewDownloadCompletedHandler, WebViewDownloadStartedHandler, WebViewDragDropEvent,
        WebViewDragDropHandler, WebViewDragDropPolicy, WebViewNavigationHandler,
        WebViewNewWindowHandler, WebViewPageLoadEvent, WebViewPageLoadHandler,
        rgba_to_webview_color,
    },
};
use anyhow::{Context as _, Result};
use gtk::prelude::*;
use parking_lot::RwLock;
#[cfg(feature = "x11")]
use raw_window_handle as rwh;
use std::{
    cell::RefCell,
    collections::HashSet,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};
use util::ResultExt;
use webkit2gtk::{CookieManagerExt, WebViewExt, WebsiteDataManagerExt};
use wry::{
    DragDropEvent as WryDragDropEvent, NewWindowResponse, PageLoadEvent, WebContext, WebView,
    WebViewBuilder, WebViewExtUnix,
};
use wry_legacy as wry;

pub(crate) struct LinuxWebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    _protocol_registration: WryCustomProtocolRegistration,
    _context: Option<WebContext>,
    rendered_url: SharedString,
    rendered_html: Option<SharedString>,
    bounds: Bounds<Pixels>,
    background_color: Option<crate::Rgba>,
    opacity: f32,
    live: Rc<RefCell<PlatformWebView>>,
    live_permission_handler: Arc<RwLock<Option<crate::webview::WebViewPermissionHandler>>>,
    live_top_level_origin: Arc<RwLock<Option<SharedString>>>,
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
    log::debug!(
        "synchronizing {} Linux X11 WebView native child surface(s)",
        webviews.len()
    );
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
            host.update_desired(webview.clone(), scale_factor);
        } else {
            let parent = X11WebViewParentHandle {
                window_id: x_window,
            };
            log::debug!(
                "creating Linux X11 WebView {} as child of native window {x_window}",
                webview.id
            );
            match LinuxWebViewHost::new(&parent, webview.clone(), scale_factor) {
                Ok(host) => {
                    log::debug!("created Linux X11 WebView {} native child", webview.id);
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
pub(crate) fn sync_wayland_webviews(_window: &WaylandWindowStatePtr, webviews: &[PlatformWebView]) {
    // A detached GTK top-level cannot provide correct Wayland placement,
    // clipping, stacking, focus, or minimize semantics for an embedded surface.
    // Do not create a visually plausible but contract-breaking overlay.
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if !webviews.is_empty() && !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::error!("Linux Wayland WebViews are unsupported; compile and select the X11 backend");
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
    _window: &WaylandWindowStatePtr,
    _command: PlatformWebViewCommand,
) -> Result<()> {
    anyhow::bail!(
        "Linux Wayland WebViews are unsupported; compile and select the X11 backend before using WebView commands"
    )
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
        let live_top_level_origin = Arc::new(RwLock::new(serialized_origin(&desired.url)));
        let protocol_registration = WryCustomProtocolRegistration::new(&desired);
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
            live_top_level_origin.clone(),
        );
        let builder = configure_wry_custom_protocols(builder, &desired, &protocol_registration);

        let webview = builder
            .build_as_child(parent)
            .context("building Linux X11 child webview")?;
        webview.set_visible(desired.visible).log_err();

        let mut host = Self {
            rendered_url: desired.url.clone(),
            rendered_html: desired.html.clone(),
            bounds: desired.bounds,
            background_color: desired.background_color,
            opacity: -1.0,
            live,
            live_permission_handler,
            live_top_level_origin,
            desired,
            webview,
            _protocol_registration: protocol_registration,
            _context: context,
        };
        host.apply(scale_factor);
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
            || self.desired.clipboard_access != webview.clipboard_access
            || self.desired.custom_protocol_schemes != webview.custom_protocol_schemes
    }

    fn update_desired(&mut self, desired: PlatformWebView, scale_factor: f32) {
        let focused_changed = self.desired.focused != desired.focused;
        *self.live.borrow_mut() = desired.clone();
        *self.live_permission_handler.write() = desired.permission_handler.clone();
        self._protocol_registration.update(&desired);
        self.desired = desired;
        self.apply(scale_factor);
        if focused_changed {
            match self.desired.focused {
                Some(true) => self.webview.focus().log_err(),
                Some(false) => self.webview.focus_parent().log_err(),
                None => None,
            };
        }
    }

    fn apply(&mut self, _scale_factor: f32) {
        if self.bounds != self.desired.bounds {
            self.webview
                .set_bounds(to_wry_rect(self.desired.bounds))
                .log_err();
            self.bounds = self.desired.bounds;
        }

        if !self.desired.url.is_empty() && self.rendered_url != self.desired.url {
            let loaded = if let Some(headers) = self.desired.request_headers.clone() {
                self.webview
                    .load_url_with_headers(self.desired.url.as_ref(), headers)
                    .log_err()
            } else {
                self.webview.load_url(self.desired.url.as_ref()).log_err()
            };
            if loaded.is_some() {
                *self.live_top_level_origin.write() = serialized_origin(&self.desired.url);
                self.rendered_url = self.desired.url.clone();
                self.rendered_html = None;
            }
        } else if self.desired.url.is_empty() && self.rendered_html != self.desired.html {
            if let Some(html) = self.desired.html.as_ref() {
                if self.webview.load_html(html.as_ref()).log_err().is_some() {
                    *self.live_top_level_origin.write() = None;
                    self.rendered_url = SharedString::default();
                    self.rendered_html = self.desired.html.clone();
                }
            } else {
                self.rendered_url = SharedString::default();
                self.rendered_html = None;
            }
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
            self.webview.webview().set_opacity(opacity);
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
                *self.live_top_level_origin.write() = serialized_origin(&url);
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                self.webview
                    .load_url_with_headers(url.as_ref(), headers)
                    .context("navigating Linux WebView with headers")?;
                *self.live_top_level_origin.write() = serialized_origin(&url);
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.webview
                    .load_html(html.as_ref())
                    .context("loading HTML into Linux WebView")?;
                *self.live_top_level_origin.write() = None;
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
                if let Err(error) =
                    self.webview
                        .evaluate_script_with_callback(script.as_ref(), move |result| {
                            super::catch_platform_callback("webview JavaScript result", (), || {
                                callback_for_result(Ok(result.into()));
                            });
                        })
                {
                    let message: SharedString = error.to_string().into();
                    super::catch_platform_callback("webview JavaScript error", (), || {
                        callback(Err(message.clone()));
                    });
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
                    .go_back()
                    .context("navigating Linux WebView backward")?;
            }
            PlatformWebViewCommand::GoForward { .. } => {
                self.webview
                    .go_forward()
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
                read_webview_cookies(&self.webview, url, callback);
            }
            PlatformWebViewCommand::SetCookie {
                cookie, callback, ..
            } => {
                set_webview_cookie(&self.webview, cookie, callback);
            }
            PlatformWebViewCommand::DeleteCookie {
                cookie, callback, ..
            } => {
                delete_webview_cookie(&self.webview, cookie, callback);
            }
        }
        Ok(())
    }
}

fn read_webview_cookies(
    webview: &WebView,
    url: Option<SharedString>,
    callback: WebViewCookieCallback,
) {
    let Some(manager) = webview_cookie_manager(webview) else {
        defer_cookie_callback(
            callback,
            Err("Linux WebView cookie manager unavailable".into()),
        );
        return;
    };
    let finish = move |result: Result<Vec<soup::Cookie>, gtk::glib::Error>| {
        let result = result
            .map(|cookies| cookies.into_iter().map(webview_cookie_from_soup).collect())
            .map_err(|error| SharedString::from(error.to_string()));
        super::catch_platform_callback("webview cookie read", (), || callback(result));
    };
    if let Some(url) = url {
        manager.cookies(url.as_ref(), None::<&gtk::gio::Cancellable>, finish);
    } else {
        linux_cookie_ffi::CookieManagerExtAll::all_cookies(
            &manager,
            None::<&gtk::gio::Cancellable>,
            finish,
        );
    }
}

fn set_webview_cookie(
    webview: &WebView,
    cookie: WebViewCookie,
    callback: WebViewCookieMutationCallback,
) {
    let Some(manager) = webview_cookie_manager(webview) else {
        defer_cookie_mutation_callback(
            callback,
            Err("Linux WebView cookie manager unavailable".into()),
        );
        return;
    };
    let mut cookie = webview_cookie_to_soup(cookie);
    manager.add_cookie(&mut cookie, None::<&gtk::gio::Cancellable>, move |result| {
        let result = result.map_err(|error| SharedString::from(error.to_string()));
        super::catch_platform_callback("webview cookie write", (), || callback(result));
    });
}

fn delete_webview_cookie(
    webview: &WebView,
    cookie: WebViewCookie,
    callback: WebViewCookieMutationCallback,
) {
    let Some(manager) = webview_cookie_manager(webview) else {
        defer_cookie_mutation_callback(
            callback,
            Err("Linux WebView cookie manager unavailable".into()),
        );
        return;
    };
    let mut cookie = webview_cookie_to_soup(cookie);
    manager.delete_cookie(&mut cookie, None::<&gtk::gio::Cancellable>, move |result| {
        let result = result.map_err(|error| SharedString::from(error.to_string()));
        super::catch_platform_callback("webview cookie deletion", (), || callback(result));
    });
}

fn webview_cookie_manager(webview: &WebView) -> Option<webkit2gtk::CookieManager> {
    webview
        .webview()
        .website_data_manager()
        .and_then(|manager| manager.cookie_manager())
}

fn defer_cookie_callback(
    callback: WebViewCookieCallback,
    result: Result<Vec<WebViewCookie>, SharedString>,
) {
    gtk::glib::idle_add_local_once(move || {
        super::catch_platform_callback("webview cookie read", (), || callback(result));
    });
}

fn defer_cookie_mutation_callback(
    callback: WebViewCookieMutationCallback,
    result: Result<(), SharedString>,
) {
    gtk::glib::idle_add_local_once(move || {
        super::catch_platform_callback("webview cookie mutation", (), || callback(result));
    });
}

fn webview_cookie_from_soup(mut cookie: soup::Cookie) -> WebViewCookie {
    WebViewCookie {
        name: cookie
            .name()
            .map(|name| name.to_string().into())
            .unwrap_or_default(),
        value: cookie
            .value()
            .map(|value| value.to_string().into())
            .unwrap_or_default(),
        domain: cookie.domain().map(|domain| domain.to_string().into()),
        path: cookie.path().map(|path| path.to_string().into()),
        secure: cookie.is_secure(),
        http_only: cookie.is_http_only(),
    }
}

fn webview_cookie_to_soup(cookie: WebViewCookie) -> soup::Cookie {
    let mut soup_cookie = soup::Cookie::new(
        cookie.name.as_ref(),
        cookie.value.as_ref(),
        cookie
            .domain
            .as_ref()
            .map(|domain| domain.as_ref())
            .unwrap_or(""),
        cookie.path.as_ref().map(|path| path.as_ref()).unwrap_or(""),
        -1,
    );
    soup_cookie.set_secure(cookie.secure);
    soup_cookie.set_http_only(cookie.http_only);
    soup_cookie
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
    live_top_level_origin: Arc<RwLock<Option<SharedString>>>,
) -> WebViewBuilder<'a> {
    let ipc_nonce = uuid::Uuid::new_v4().simple().to_string();
    if desired.storage_key.is_none() {
        builder = builder.with_incognito(true);
    }

    let permission_origin = live_top_level_origin.clone();
    builder = builder.with_permission_handler(move |kind| {
        let handler = live_permission_handler.read().clone();
        handler
            .map(|handler| {
                let decision = super::catch_platform_callback(
                    "webview native permission policy",
                    crate::WebViewPermissionDecision::Deny,
                    || {
                        handler(
                            crate::WebViewNativePermissionRequest::with_top_level_origin(
                                permission_kind_from_wry(kind),
                                permission_origin.read().clone(),
                            ),
                        )
                    },
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
    let ipc_nonce_for_handler = ipc_nonce.clone();
    let ipc_origin = live_top_level_origin.clone();
    let rejected_ipc_reported = AtomicBool::new(false);
    builder = builder.with_ipc_handler(move |request| {
        let (handler, mut async_window) = {
            let live = ipc_live.borrow();
            (live.message_handler.clone(), live.async_window.clone())
        };
        let Some(handler) = handler else {
            return;
        };

        let expected_origin = ipc_origin.read().clone();
        if !ipc_source_matches_top_level(
            &request.uri().to_string(),
            expected_origin.as_ref().map(|origin| origin.as_ref()),
        ) {
            warn_rejected_ipc_once(&rejected_ipc_reported, "Linux");
            return;
        }
        let Some(payload) = decode_bridge_message(request.body(), &ipc_nonce_for_handler) else {
            warn_rejected_ipc_once(&rejected_ipc_reported, "Linux");
            return;
        };
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

    builder = builder.with_initialization_script_for_main_only(
        bridge_script(desired.storage_key.as_ref(), &ipc_nonce),
        true,
    );
    for css in &desired.injected_css {
        builder = builder.with_initialization_script_for_main_only(
            main_frame_script(&css_script(css.as_ref())),
            true,
        );
    }
    for javascript in &desired.injected_javascript {
        builder = builder
            .with_initialization_script_for_main_only(main_frame_script(javascript.as_ref()), true);
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
    let page_load_origin = live_top_level_origin;
    builder = builder.with_on_page_load_handler(move |event, url| {
        *page_load_origin.write() = serialized_origin(&url);
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

mod linux_cookie_ffi {
    use gtk::{
        gio::{self, Cancellable, ffi::GAsyncReadyCallback},
        glib::{
            self,
            prelude::IsA,
            translate::{FromGlibPtrContainer, ToGlibPtr},
        },
    };
    use webkit2gtk::CookieManager;

    pub(super) trait CookieManagerExtAll: IsA<CookieManager> + 'static {
        fn all_cookies<P: FnOnce(Result<Vec<soup::Cookie>, glib::Error>) + 'static>(
            &self,
            cancellable: Option<&impl IsA<Cancellable>>,
            callback: P,
        ) {
            let main_context = glib::MainContext::ref_thread_default();
            let owns_context = main_context.is_owner();
            let acquired_context = (!owns_context)
                .then(|| main_context.acquire().ok())
                .flatten();
            assert!(
                owns_context || acquired_context.is_some(),
                "WebView cookie operations require ownership of the GTK main context"
            );

            let user_data: Box<glib::thread_guard::ThreadGuard<P>> =
                Box::new(glib::thread_guard::ThreadGuard::new(callback));
            unsafe extern "C" fn trampoline<
                P: FnOnce(Result<Vec<soup::Cookie>, glib::Error>) + 'static,
            >(
                source: *mut glib::gobject_ffi::GObject,
                result: *mut gio::ffi::GAsyncResult,
                user_data: glib::ffi::gpointer,
            ) {
                let mut error = std::ptr::null_mut();
                let cookies = unsafe {
                    webkit_cookie_manager_get_all_cookies_finish(source.cast(), result, &mut error)
                };
                let result = if error.is_null() {
                    Ok(unsafe { FromGlibPtrContainer::from_glib_full(cookies) })
                } else {
                    Err(unsafe { glib::translate::from_glib_full(error) })
                };
                let callback: Box<glib::thread_guard::ThreadGuard<P>> =
                    unsafe { Box::from_raw(user_data.cast()) };
                callback.into_inner()(result);
            }

            unsafe {
                webkit_cookie_manager_get_all_cookies(
                    self.as_ref().to_glib_none().0,
                    cancellable.map(|value| value.as_ref()).to_glib_none().0,
                    Some(trampoline::<P>),
                    Box::into_raw(user_data).cast(),
                );
            }
        }
    }

    impl CookieManagerExtAll for CookieManager {}

    unsafe extern "C" {
        fn webkit_cookie_manager_get_all_cookies(
            cookie_manager: *mut webkit2gtk_sys::WebKitCookieManager,
            cancellable: *mut gio::ffi::GCancellable,
            callback: GAsyncReadyCallback,
            user_data: glib::ffi::gpointer,
        );

        fn webkit_cookie_manager_get_all_cookies_finish(
            cookie_manager: *mut webkit2gtk_sys::WebKitCookieManager,
            result: *mut gio::ffi::GAsyncResult,
            error: *mut *mut glib::ffi::GError,
        ) -> *mut glib::ffi::GList;
    }
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
