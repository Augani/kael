//! WebKitGTK 6 host for the GTK-owned native Wayland window backend.
//!
//! WebViews are regular GTK4 children in the same widget/render tree as the
//! Kael GSK scene. This gives Wayland correct stacking, clipping, focus,
//! accessibility and window lifecycle semantics without a detached overlay.

use super::catch_platform_callback;
use crate::{
    AsyncWindowContext, Bounds, Pixels, Point, Rgba, SharedString, WebViewCookie,
    WebViewDownloadCompleted, WebViewDownloadPolicy, WebViewNewWindowPolicy,
    platform::webview_common::{
        decode_bridge_message, json_string_literal, serialized_origin, webview_command_id,
        webview_storage_dir,
    },
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand, WebViewCookieCallback,
        WebViewCookieMutationCallback, WebViewDragDropEvent, WebViewDragDropPolicy,
        WebViewNavigationHandler, WebViewNewWindowHandler, WebViewPageLoadEvent,
        WebViewPermissionDecision, WebViewPermissionKind,
    },
};
use anyhow::{Context as _, Result};
use collections::{FxHashMap, FxHashSet};
use gtk4::{Fixed, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};
use webkit6::{
    ClipboardPermissionRequest, Download, GeolocationPermissionRequest, LoadEvent,
    MediaKeySystemPermissionRequest, NavigationPolicyDecision, NetworkSession,
    NotificationPermissionRequest, PermissionRequest, PointerLockPermissionRequest,
    PolicyDecisionType, PrintOperation, Settings, URIRequest, URISchemeRequest, URISchemeResponse,
    UserContentInjectedFrames, UserContentManager, UserMediaPermissionRequest, UserScript,
    UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebView,
    WebsiteDataAccessPermissionRequest, WebsiteDataTypes, prelude::*,
};

thread_local! {
    /// Persistent profiles with the same storage key must share one WebKit
    /// network session. Ephemeral profiles intentionally remain per-host.
    static PERSISTENT_SESSIONS: RefCell<FxHashMap<SharedString, NetworkSession>> =
        RefCell::new(FxHashMap::default());
    /// WebKit registers URI schemes on a shared `WebContext`. Keep one live
    /// application context per context/scheme and refresh it as windows paint,
    /// so a persistent profile never retains a closed first window forever.
    static REGISTERED_PROTOCOLS: RefCell<FxHashMap<(usize, SharedString), Rc<RefCell<AsyncWindowContext>>>> =
        RefCell::new(FxHashMap::default());
}

pub(crate) struct Gtk4WebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    session: NetworkSession,
    download_signal: Option<glib::SignalHandlerId>,
    rendered_url: SharedString,
    rendered_html: Option<SharedString>,
    bounds: Bounds<Pixels>,
    background_color: Option<Rgba>,
    opacity: f32,
    live: Rc<RefCell<PlatformWebView>>,
    top_level_origin: Rc<RefCell<Option<SharedString>>>,
}

impl Drop for Gtk4WebViewHost {
    fn drop(&mut self) {
        if let Some(signal) = self.download_signal.take() {
            self.session.disconnect(signal);
        }
    }
}

impl Gtk4WebViewHost {
    fn new(fixed: &Fixed, desired: PlatformWebView) -> Result<Self> {
        let manager = UserContentManager::new();
        anyhow::ensure!(
            manager.register_script_message_handler("kael", None),
            "registering the WebKitGTK 6 script-message bridge"
        );

        let nonce = uuid::Uuid::new_v4().simple().to_string();
        manager.add_script(&UserScript::new(
            &webkit_bridge_script(desired.storage_key.as_ref(), &nonce),
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        for css in &desired.injected_css {
            manager.add_style_sheet(&UserStyleSheet::new(
                css,
                UserContentInjectedFrames::TopFrame,
                UserStyleLevel::User,
                &[],
                &[],
            ));
        }
        for script in &desired.injected_javascript {
            manager.add_script(&UserScript::new(
                script,
                UserContentInjectedFrames::TopFrame,
                UserScriptInjectionTime::Start,
                &[],
                &[],
            ));
        }

        let session = network_session(&desired)?;
        let settings = Settings::new();
        settings.set_enable_javascript(!desired.javascript_disabled);
        settings.set_enable_developer_extras(desired.devtools);
        settings.set_javascript_can_access_clipboard(desired.clipboard_access);
        if let Some(user_agent) = desired.user_agent.as_ref() {
            settings.set_user_agent(Some(user_agent));
        }
        if let Some(autoplay) = desired.media_autoplay {
            settings.set_media_playback_requires_user_gesture(!autoplay);
        }

        let webview = WebView::builder()
            .network_session(&session)
            .settings(&settings)
            .user_content_manager(&manager)
            .build();
        webview.set_hexpand(false);
        webview.set_vexpand(false);
        register_custom_protocols(&webview, &desired)?;

        let live = Rc::new(RefCell::new(desired.clone()));
        let top_level_origin = Rc::new(RefCell::new(serialized_origin(&desired.url)));
        configure_callbacks(
            &webview,
            &manager,
            live.clone(),
            top_level_origin.clone(),
            nonce,
        );
        install_webview_drag_drop(&webview, live.clone());
        let download_signal = Some(configure_downloads(&session, &webview, live.clone()));

        let initial_bounds = desired.bounds;
        fixed.put(
            &webview,
            finite_coordinate(initial_bounds.origin.x.0),
            finite_coordinate(initial_bounds.origin.y.0),
        );

        let mut host = Self {
            desired: desired.clone(),
            webview,
            session,
            download_signal,
            rendered_url: SharedString::default(),
            rendered_html: None,
            bounds: Bounds::default(),
            background_color: None,
            opacity: -1.0,
            live,
            top_level_origin,
        };
        host.apply(fixed, true)?;
        Ok(host)
    }

    fn needs_recreate(&self, desired: &PlatformWebView) -> bool {
        self.desired.storage_key != desired.storage_key
            || self.desired.user_agent != desired.user_agent
            || self.desired.injected_css != desired.injected_css
            || self.desired.injected_javascript != desired.injected_javascript
            || self.desired.javascript_disabled != desired.javascript_disabled
            || self.desired.devtools != desired.devtools
            || self.desired.zoom_hotkeys_enabled != desired.zoom_hotkeys_enabled
            || self.desired.media_autoplay != desired.media_autoplay
            || self.desired.clipboard_access != desired.clipboard_access
    }

    fn update_desired(&mut self, fixed: &Fixed, desired: PlatformWebView) -> Result<()> {
        let focus_changed = self.desired.focused != desired.focused;
        register_custom_protocols(&self.webview, &desired)?;
        *self.live.borrow_mut() = desired.clone();
        self.desired = desired;
        self.apply(fixed, false)?;
        if focus_changed {
            match self.desired.focused {
                Some(true) => {
                    self.webview.grab_focus();
                }
                Some(false) => focus_parent(&self.webview),
                None => {}
            }
        }
        Ok(())
    }

    fn apply(&mut self, fixed: &Fixed, initial: bool) -> Result<()> {
        let bounds = sanitize_bounds(self.desired.bounds);
        if initial || self.bounds != bounds {
            fixed.move_(
                &self.webview,
                finite_coordinate(bounds.origin.x.0),
                finite_coordinate(bounds.origin.y.0),
            );
            self.webview.set_size_request(
                finite_extent(bounds.size.width.0),
                finite_extent(bounds.size.height.0),
            );
            self.bounds = bounds;
        }

        if !self.desired.url.is_empty() && (initial || self.rendered_url != self.desired.url) {
            load_uri(
                &self.webview,
                &self.desired.url,
                self.desired.request_headers.as_ref(),
            )?;
            *self.top_level_origin.borrow_mut() = serialized_origin(&self.desired.url);
            self.rendered_url = self.desired.url.clone();
            self.rendered_html = None;
        } else if self.desired.url.is_empty()
            && (initial || self.rendered_html != self.desired.html)
        {
            if let Some(html) = self.desired.html.as_ref() {
                self.webview.load_html(html, None);
            }
            *self.top_level_origin.borrow_mut() = None;
            self.rendered_url = SharedString::default();
            self.rendered_html = self.desired.html.clone();
        }

        if initial || self.background_color != self.desired.background_color {
            self.webview
                .set_background_color(&gdk_rgba(self.desired.background_color.unwrap_or(Rgba {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                })));
            self.background_color = self.desired.background_color;
        }
        if initial || self.opacity != self.desired.opacity {
            self.webview
                .set_opacity(self.desired.opacity.clamp(0.0, 1.0) as f64);
            self.opacity = self.desired.opacity;
        }
        self.webview.set_visible(self.desired.visible);
        if initial && self.desired.focused == Some(true) {
            self.webview.grab_focus();
        }
        Ok(())
    }

    fn apply_command(&mut self, command: PlatformWebViewCommand) -> Result<()> {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.webview.load_uri(&url);
                *self.top_level_origin.borrow_mut() = serialized_origin(&url);
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                load_uri(&self.webview, &url, Some(&headers))?;
                *self.top_level_origin.borrow_mut() = serialized_origin(&url);
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.webview.load_html(&html, None);
                *self.top_level_origin.borrow_mut() = None;
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.webview.evaluate_javascript(
                    &script,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    |result| {
                        if let Err(error) = result {
                            log::warn!("WebKitGTK 6 JavaScript evaluation failed: {error}");
                        }
                    },
                );
            }
            PlatformWebViewCommand::EvaluateJavaScriptWithResult {
                script, callback, ..
            } => {
                self.webview.evaluate_javascript(
                    &script,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    move |result| {
                        let result = result
                            .map(|value| javascript_result(&value).into())
                            .map_err(|error| error.to_string().into());
                        catch_platform_callback("webview JavaScript result", (), || {
                            callback(result)
                        });
                    },
                );
            }
            PlatformWebViewCommand::PostMessage { message, .. } => {
                let payload = serde_json::to_string(&message)
                    .context("serializing WebKitGTK 6 host message")?;
                let script = format!(
                    "(() => {{ const payload = {payload}; window.dispatchEvent?.(new MessageEvent('message', {{ data: payload }})); if (typeof window.onmessage === 'function') window.onmessage({{ data: payload }}); }})();"
                );
                self.webview.evaluate_javascript(
                    &script,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    |_| {},
                );
            }
            PlatformWebViewCommand::Reload { .. } => self.webview.reload(),
            PlatformWebViewCommand::GoBack { .. } => self.webview.go_back(),
            PlatformWebViewCommand::GoForward { .. } => self.webview.go_forward(),
            PlatformWebViewCommand::OpenDevTools { .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                self.webview
                    .inspector()
                    .context("WebKitGTK 6 inspector unavailable")?
                    .show();
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                anyhow::bail!("Linux WebView devtools require a debug build or `devtools` feature");
            }
            PlatformWebViewCommand::CloseDevTools { .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                self.webview
                    .inspector()
                    .context("WebKitGTK 6 inspector unavailable")?
                    .close();
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                anyhow::bail!("Linux WebView devtools require a debug build or `devtools` feature");
            }
            PlatformWebViewCommand::IsDevToolsOpen { callback, .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                callback(Ok(self
                    .webview
                    .inspector()
                    .is_some_and(|inspector| inspector.is_attached())));
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                callback(Err(
                    "WebView devtools state requires a debug build or backend devtools support"
                        .into(),
                ));
            }
            PlatformWebViewCommand::Print { .. } => PrintOperation::new(&self.webview).print(),
            PlatformWebViewCommand::SetZoomFactor { factor, .. } => {
                anyhow::ensure!(
                    factor.is_finite() && factor > 0.0,
                    "invalid WebView zoom factor"
                );
                self.webview.set_zoom_level(factor);
            }
            PlatformWebViewCommand::Focus { .. } => {
                self.webview.grab_focus();
            }
            PlatformWebViewCommand::FocusParent { .. } => focus_parent(&self.webview),
            PlatformWebViewCommand::ClearBrowsingData { .. } => {
                let manager = self
                    .session
                    .website_data_manager()
                    .context("WebKitGTK 6 website data manager unavailable")?;
                manager.clear(
                    WebsiteDataTypes::ALL,
                    glib::TimeSpan::from_microseconds(0),
                    None::<&gio::Cancellable>,
                    |result| {
                        if let Err(error) = result {
                            log::warn!("clearing WebKitGTK 6 browsing data failed: {error}");
                        }
                    },
                );
            }
            PlatformWebViewCommand::ReadUrl { callback, .. } => callback(Ok(self
                .webview
                .uri()
                .map_or_else(SharedString::default, |url| url.to_string().into()))),
            PlatformWebViewCommand::ReadCookies { url, callback, .. } => {
                read_cookies(&self.session, url, callback)
            }
            PlatformWebViewCommand::SetCookie {
                cookie, callback, ..
            } => mutate_cookie(&self.session, cookie, callback, false),
            PlatformWebViewCommand::DeleteCookie {
                cookie, callback, ..
            } => mutate_cookie(&self.session, cookie, callback, true),
        }
        Ok(())
    }
}

pub(crate) fn sync_webviews(
    fixed: &Fixed,
    hosts: &mut FxHashMap<SharedString, Gtk4WebViewHost>,
    webviews: &[PlatformWebView],
) {
    let mut active = FxHashSet::default();
    for desired in webviews {
        let instance_id = desired.instance_id.clone();
        active.insert(instance_id.clone());

        let recreate = hosts
            .get(&instance_id)
            .is_some_and(|host| host.needs_recreate(desired));
        if recreate {
            if let Some(host) = hosts.remove(&instance_id) {
                fixed.remove(&host.webview);
            }
        }

        if let Some(host) = hosts.get_mut(&instance_id) {
            if let Err(error) = host.update_desired(fixed, desired.clone()) {
                log::error!(
                    "updating WebKitGTK 6 WebView {} failed: {error:#}",
                    desired.id
                );
            }
        } else {
            match Gtk4WebViewHost::new(fixed, desired.clone()) {
                Ok(host) => {
                    hosts.insert(instance_id, host);
                }
                Err(error) => {
                    log::error!(
                        "creating WebKitGTK 6 WebView {} failed: {error:#}",
                        desired.id
                    );
                }
            }
        }
    }

    let stale = hosts
        .keys()
        .filter(|id| !active.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    for id in stale {
        if let Some(host) = hosts.remove(&id) {
            fixed.remove(&host.webview);
        }
    }
}

pub(crate) fn dispatch_webview_command(
    hosts: &mut FxHashMap<SharedString, Gtk4WebViewHost>,
    command: PlatformWebViewCommand,
) -> Result<()> {
    let id = webview_command_id(&command);
    let mut matches = hosts.values_mut().filter(|host| host.desired.id == id);
    let Some(host) = matches.next() else {
        anyhow::bail!("unknown webview: {id}");
    };
    anyhow::ensure!(
        matches.next().is_none(),
        "ambiguous webview id `{id}`; WebView command ids must be unique within a window"
    );
    host.apply_command(command)
}

/// Whether a visible WebView owns the given point in window coordinates.
///
/// GTK event controllers installed on the shared parent see bubbled child
/// events. Filtering against the declarative WebView bounds keeps Kael's scene
/// input from firing underneath interactive web content.
pub(crate) fn webview_intercepts_pointer(
    hosts: &FxHashMap<SharedString, Gtk4WebViewHost>,
    point: Point<Pixels>,
) -> bool {
    hosts.values().any(|host| {
        host.desired.visible
            && host.desired.opacity > 0.0
            && sanitize_bounds(host.desired.bounds).contains(&point)
    })
}

/// Whether keyboard focus currently lives in an embedded WebView.
pub(crate) fn webview_owns_focus(
    hosts: &FxHashMap<SharedString, Gtk4WebViewHost>,
    focus: Option<&gtk4::Widget>,
) -> bool {
    let Some(focus) = focus else { return false };
    hosts.values().any(|host| {
        host.webview.upcast_ref::<gtk4::Widget>() == focus || host.webview.is_ancestor(focus)
    })
}

fn register_custom_protocols(webview: &WebView, desired: &PlatformWebView) -> Result<()> {
    if desired.custom_protocol_schemes.is_empty() {
        return Ok(());
    }
    let context = webview
        .web_context()
        .context("WebKitGTK 6 WebContext unavailable for custom protocols")?;
    let context_id = context.as_ptr() as usize;

    for scheme in &desired.custom_protocol_schemes {
        let key = (context_id, scheme.clone());
        let existing = REGISTERED_PROTOCOLS.with(|protocols| protocols.borrow().get(&key).cloned());
        if let Some(existing) = existing {
            *existing.borrow_mut() = desired.async_window.clone();
            continue;
        }

        let live_context = Rc::new(RefCell::new(desired.async_window.clone()));
        let request_context = live_context.clone();
        context.register_uri_scheme(scheme, move |request| {
            handle_custom_protocol_request(request, &request_context)
        });
        if let Some(security) = context.security_manager() {
            security.register_uri_scheme_as_local(scheme);
            security.register_uri_scheme_as_secure(scheme);
            security.register_uri_scheme_as_cors_enabled(scheme);
        }
        REGISTERED_PROTOCOLS.with(|protocols| {
            protocols.borrow_mut().insert(key, live_context);
        });
    }
    Ok(())
}

fn handle_custom_protocol_request(
    request: &URISchemeRequest,
    live_context: &Rc<RefCell<AsyncWindowContext>>,
) {
    let Some(uri) = request.uri().map(|uri| uri.to_string()) else {
        finish_custom_protocol_error(request, 400, "Bad Request");
        return;
    };
    let result = live_context
        .borrow_mut()
        .update(|_, cx| cx.handle_custom_protocol_url(uri));
    match result {
        Ok(Ok(Some(response))) => finish_custom_protocol_response(request, response),
        Ok(Ok(None)) => finish_custom_protocol_error(request, 404, "Not Found"),
        Ok(Err(error)) | Err(error) => {
            log::warn!("serving WebKitGTK 6 custom protocol failed: {error:#}");
            finish_custom_protocol_error(request, 500, "Internal Server Error");
        }
    }
}

fn finish_custom_protocol_response(
    request: &URISchemeRequest,
    response: crate::CustomProtocolResponse,
) {
    let bytes = glib::Bytes::from_owned(response.body);
    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    let webkit_response = URISchemeResponse::new(&stream, bytes.len() as i64);
    webkit_response.set_status(u32::from(response.status), None);
    webkit_response.set_content_type(&response.mime_type);
    if !response.headers.is_empty() {
        let headers =
            webkit6::soup::MessageHeaders::new(webkit6::soup::MessageHeadersType::Response);
        for (name, value) in response.headers {
            headers.append(&name, &value);
        }
        webkit_response.set_http_headers(headers);
    }
    request.finish_with_response(&webkit_response);
}

fn finish_custom_protocol_error(request: &URISchemeRequest, status: u16, message: &'static str) {
    finish_custom_protocol_response(
        request,
        crate::CustomProtocolResponse {
            status,
            mime_type: "text/plain; charset=utf-8".to_string(),
            headers: vec![("Cache-Control".to_string(), "no-store".to_string())],
            body: message.as_bytes().to_vec(),
        },
    );
}

fn install_webview_drag_drop(webview: &WebView, live: Rc<RefCell<PlatformWebView>>) {
    let target = gtk4::DropTarget::new(
        gtk4::gdk::FileList::static_type(),
        gtk4::gdk::DragAction::COPY,
    );
    target.set_preload(true);
    target.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let active = Rc::new(Cell::new(false));
    let active_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));

    let enter_live = live.clone();
    let enter_webview = webview.clone();
    let enter_active = active.clone();
    let enter_paths = active_paths.clone();
    target.connect_enter(move |target, x, y| {
        let paths = target
            .value()
            .and_then(|value| value.get::<gtk4::gdk::FileList>().ok())
            .map(file_list_paths)
            .unwrap_or_default();
        if paths.is_empty() || enter_live.borrow().drag_drop_handler.is_none() {
            return gtk4::gdk::DragAction::empty();
        }
        enter_active.set(true);
        *enter_paths.borrow_mut() = paths.clone();
        let event = WebViewDragDropEvent::Enter {
            paths,
            position: webview_drag_position(&enter_webview, x, y),
        };
        drag_action_for_policy(dispatch_webview_drag_drop(&enter_live, event))
    });

    let motion_live = live.clone();
    let motion_webview = webview.clone();
    let motion_active = active.clone();
    target.connect_motion(move |_, x, y| {
        if !motion_active.get() || motion_live.borrow().drag_drop_handler.is_none() {
            return gtk4::gdk::DragAction::empty();
        }
        drag_action_for_policy(dispatch_webview_drag_drop(
            &motion_live,
            WebViewDragDropEvent::Over {
                position: webview_drag_position(&motion_webview, x, y),
            },
        ))
    });

    let leave_live = live.clone();
    let leave_active = active.clone();
    let leave_paths = active_paths.clone();
    target.connect_leave(move |_| {
        if leave_active.replace(false) {
            leave_paths.borrow_mut().clear();
            let _ = dispatch_webview_drag_drop(&leave_live, WebViewDragDropEvent::Leave);
        }
    });

    let drop_live = live;
    let drop_webview = webview.clone();
    let drop_active = active;
    let drop_paths = active_paths;
    target.connect_drop(move |_, value, x, y| {
        let paths = value
            .get::<gtk4::gdk::FileList>()
            .ok()
            .map(file_list_paths)
            .filter(|paths| !paths.is_empty())
            .unwrap_or_else(|| drop_paths.borrow().clone());
        drop_active.set(false);
        drop_paths.borrow_mut().clear();
        if paths.is_empty() || drop_live.borrow().drag_drop_handler.is_none() {
            return false;
        }
        dispatch_webview_drag_drop(
            &drop_live,
            WebViewDragDropEvent::Drop {
                paths,
                position: webview_drag_position(&drop_webview, x, y),
            },
        ) == WebViewDragDropPolicy::BlockBrowserDefault
    });
    webview.add_controller(target);
}

fn file_list_paths(files: gtk4::gdk::FileList) -> Vec<PathBuf> {
    files
        .files()
        .into_iter()
        .filter_map(|file| file.path())
        .collect()
}

fn webview_drag_position(webview: &WebView, x: f64, y: f64) -> (i32, i32) {
    let scale = f64::from(webview.scale_factor().max(1));
    let coordinate = |value: f64| {
        let value = if value.is_finite() { value } else { 0.0 };
        (value * scale)
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    (coordinate(x), coordinate(y))
}

fn dispatch_webview_drag_drop(
    live: &Rc<RefCell<PlatformWebView>>,
    event: WebViewDragDropEvent,
) -> WebViewDragDropPolicy {
    let (handler, mut async_window) = {
        let live = live.borrow();
        (live.drag_drop_handler.clone(), live.async_window.clone())
    };
    let Some(handler) = handler else {
        return WebViewDragDropPolicy::AllowBrowserDefault;
    };
    catch_platform_callback(
        "webview drag-and-drop",
        WebViewDragDropPolicy::BlockBrowserDefault,
        || {
            async_window
                .update(|window, cx| handler(event, window, cx))
                .unwrap_or(WebViewDragDropPolicy::BlockBrowserDefault)
        },
    )
}

fn drag_action_for_policy(policy: WebViewDragDropPolicy) -> gtk4::gdk::DragAction {
    match policy {
        WebViewDragDropPolicy::AllowBrowserDefault => gtk4::gdk::DragAction::empty(),
        WebViewDragDropPolicy::BlockBrowserDefault => gtk4::gdk::DragAction::COPY,
    }
}

fn configure_callbacks(
    webview: &WebView,
    manager: &UserContentManager,
    live: Rc<RefCell<PlatformWebView>>,
    top_level_origin: Rc<RefCell<Option<SharedString>>>,
    nonce: String,
) {
    let ipc_live = live.clone();
    let rejected_ipc = Rc::new(Cell::new(false));
    manager.connect_script_message_received(Some("kael"), move |_, value| {
        let Some(payload) = decode_bridge_message(value.to_str().as_str(), &nonce) else {
            if !rejected_ipc.replace(true) {
                log::warn!(
                    "rejected untrusted Linux WebKitGTK 6 IPC message; further rejections are suppressed"
                );
            }
            return;
        };
        let (handler, mut async_window) = {
            let live = ipc_live.borrow();
            (live.message_handler.clone(), live.async_window.clone())
        };
        if let Some(handler) = handler {
            catch_platform_callback("webview message", (), || {
                let _ = async_window.update(|window, cx| handler(payload, window, cx));
            });
        }
    });

    let navigation_live = live.clone();
    webview.connect_decide_policy(move |webview, decision, kind| {
        let Some(decision) = decision.downcast_ref::<NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = decision.navigation_action() else {
            return false;
        };
        let Some(url) = action.request().and_then(|request| request.uri()) else {
            return false;
        };
        let url = url.to_string();
        match kind {
            PolicyDecisionType::NavigationAction => {
                let (handler, mut async_window) = {
                    let live = navigation_live.borrow();
                    (live.navigation_handler.clone(), live.async_window.clone())
                };
                if let Some(handler) = handler {
                    let allow = catch_platform_callback(
                        "webview navigation policy",
                        NavigationPolicy::Deny,
                        || {
                            async_window
                                .update(|window, cx| handler(url.clone().into(), window, cx))
                                .unwrap_or(NavigationPolicy::Deny)
                        },
                    ) == NavigationPolicy::Allow;
                    if allow {
                        decision.use_();
                    } else {
                        decision.ignore();
                    }
                    true
                } else {
                    false
                }
            }
            PolicyDecisionType::NewWindowAction => {
                let policy = resolve_new_window_policy(url.as_str(), &navigation_live.borrow());
                match policy {
                    WebViewNewWindowPolicy::Deny => decision.ignore(),
                    WebViewNewWindowPolicy::NavigateCurrent => {
                        decision.ignore();
                        webview.load_uri(&url);
                    }
                    WebViewNewWindowPolicy::Allow => {
                        // Kael's callback does not supply a child-window
                        // factory. Make the allowed request visible through
                        // the desktop URL handler instead of approving a
                        // WebKit `create` signal that has no view to return.
                        decision.ignore();
                        if let Err(error) = gio::AppInfo::launch_default_for_uri(
                            &url,
                            None::<&gio::AppLaunchContext>,
                        ) {
                            log::warn!(
                                "opening allowed WebKitGTK 6 new-window URL failed: {error}"
                            );
                        }
                    }
                }
                true
            }
            _ => false,
        }
    });

    let load_live = live.clone();
    let load_origin = top_level_origin.clone();
    webview.connect_load_changed(move |webview, event| {
        let event = match event {
            LoadEvent::Started => WebViewPageLoadEvent::Started,
            LoadEvent::Finished => WebViewPageLoadEvent::Finished,
            _ => return,
        };
        let url: SharedString = webview
            .uri()
            .map_or_else(SharedString::default, |url| url.to_string().into());
        *load_origin.borrow_mut() = serialized_origin(&url);
        let (handler, mut async_window) = {
            let live = load_live.borrow();
            (live.page_load_handler.clone(), live.async_window.clone())
        };
        if let Some(handler) = handler {
            catch_platform_callback("webview page load", (), || {
                let _ = async_window.update(|window, cx| handler(event, url, window, cx));
            });
        }
    });

    let title_live = live.clone();
    webview.connect_title_notify(move |webview| {
        let Some(title) = webview.title() else {
            return;
        };
        let title = title.to_string();
        let (handler, mut async_window) = {
            let live = title_live.borrow();
            (
                live.document_title_changed_handler.clone(),
                live.async_window.clone(),
            )
        };
        if let Some(handler) = handler {
            catch_platform_callback("webview title change", (), || {
                let _ = async_window.update(|window, cx| handler(title.clone().into(), window, cx));
            });
        }
    });

    let permission_live = live;
    webview.connect_permission_request(move |_, request| {
        let (handler, origin) = {
            let live = permission_live.borrow();
            (
                live.permission_handler.clone(),
                top_level_origin.borrow().clone(),
            )
        };
        let Some(handler) = handler else {
            return false;
        };
        let kind = permission_kind(request);
        let decision = catch_platform_callback(
            "webview native permission policy",
            WebViewPermissionDecision::Deny,
            || handler(crate::WebViewNativePermissionRequest::with_top_level_origin(kind, origin)),
        );
        match decision {
            WebViewPermissionDecision::Allow => request.allow(),
            WebViewPermissionDecision::Deny => request.deny(),
            WebViewPermissionDecision::Default => return false,
        }
        true
    });
}

fn configure_downloads(
    session: &NetworkSession,
    webview: &WebView,
    live: Rc<RefCell<PlatformWebView>>,
) -> glib::SignalHandlerId {
    let expected_webview = webview.clone();
    session.connect_download_started(move |_, download| {
        if download.web_view().as_ref() != Some(&expected_webview) {
            return;
        }
        configure_download(download, live.clone());
    })
}

fn configure_download(download: &Download, live: Rc<RefCell<PlatformWebView>>) {
    let url: SharedString = download
        .request()
        .and_then(|request| request.uri())
        .map_or_else(SharedString::default, |url| url.to_string().into());
    let destination = Rc::new(RefCell::new(None::<PathBuf>));
    let terminal = Rc::new(Cell::new(false));

    let decide_live = live.clone();
    let decide_url = url.clone();
    let decide_destination = destination.clone();
    download.connect_decide_destination(move |download, suggested| {
        let (handler, mut async_window) = {
            let live = decide_live.borrow();
            (
                live.download_started_handler.clone(),
                live.async_window.clone(),
            )
        };
        let Some(handler) = handler else {
            return false;
        };
        let suggested_path = (!suggested.is_empty()).then(|| PathBuf::from(suggested));
        let policy = catch_platform_callback(
            "webview download started",
            WebViewDownloadPolicy::Deny,
            || {
                async_window
                    .update(|window, cx| handler(decide_url.clone(), suggested_path, window, cx))
                    .unwrap_or(WebViewDownloadPolicy::Deny)
            },
        );
        match policy {
            WebViewDownloadPolicy::Allow => false,
            WebViewDownloadPolicy::Deny => {
                download.cancel();
                true
            }
            WebViewDownloadPolicy::SaveTo(path) if path.is_absolute() => {
                download.set_destination(&gio::File::for_path(&path).uri());
                *decide_destination.borrow_mut() = Some(path);
                true
            }
            WebViewDownloadPolicy::SaveTo(path) => {
                log::warn!(
                    "WebView download destination must be absolute: {}",
                    path.display()
                );
                download.cancel();
                true
            }
        }
    });

    let finished_live = live.clone();
    let finished_url = url.clone();
    let finished_destination = destination.clone();
    let finished_terminal = terminal.clone();
    download.connect_finished(move |download| {
        if finished_terminal.replace(true) {
            return;
        }
        let path = finished_destination.borrow().clone().or_else(|| {
            download
                .destination()
                .and_then(|uri| gio::File::for_uri(uri.as_str()).path())
        });
        dispatch_download_completed(
            &finished_live,
            WebViewDownloadCompleted {
                url: finished_url.clone(),
                path,
                success: true,
            },
        );
    });

    let failed_live = live;
    let failed_terminal = terminal;
    download.connect_failed(move |download, error| {
        if failed_terminal.replace(true) {
            return;
        }
        log::warn!("WebKitGTK 6 download failed: {error}");
        let path = download
            .destination()
            .and_then(|uri| gio::File::for_uri(uri.as_str()).path());
        dispatch_download_completed(
            &failed_live,
            WebViewDownloadCompleted {
                url: url.clone(),
                path,
                success: false,
            },
        );
    });
}

fn dispatch_download_completed(
    live: &Rc<RefCell<PlatformWebView>>,
    event: WebViewDownloadCompleted,
) {
    let (handler, mut async_window) = {
        let live = live.borrow();
        (
            live.download_completed_handler.clone(),
            live.async_window.clone(),
        )
    };
    if let Some(handler) = handler {
        catch_platform_callback("webview download completed", (), || {
            let _ = async_window.update(|window, cx| handler(event, window, cx));
        });
    }
}

fn resolve_new_window_policy(url: &str, live: &PlatformWebView) -> WebViewNewWindowPolicy {
    if let Some(handler) = live.new_window_handler.clone() {
        return call_new_window_handler(url, handler, live.async_window.clone());
    }
    if let Some(handler) = live.navigation_handler.clone() {
        return if call_navigation_handler(url, handler, live.async_window.clone())
            == NavigationPolicy::Allow
        {
            WebViewNewWindowPolicy::NavigateCurrent
        } else {
            WebViewNewWindowPolicy::Deny
        };
    }
    WebViewNewWindowPolicy::NavigateCurrent
}

fn call_new_window_handler(
    url: &str,
    handler: WebViewNewWindowHandler,
    mut async_window: AsyncWindowContext,
) -> WebViewNewWindowPolicy {
    let url = url.to_owned();
    catch_platform_callback(
        "webview new-window policy",
        WebViewNewWindowPolicy::Deny,
        || {
            async_window
                .update(|window, cx| handler(url.clone().into(), window, cx))
                .unwrap_or(WebViewNewWindowPolicy::Deny)
        },
    )
}

fn call_navigation_handler(
    url: &str,
    handler: WebViewNavigationHandler,
    mut async_window: AsyncWindowContext,
) -> NavigationPolicy {
    let url = url.to_owned();
    catch_platform_callback("webview navigation policy", NavigationPolicy::Deny, || {
        async_window
            .update(|window, cx| handler(url.clone().into(), window, cx))
            .unwrap_or(NavigationPolicy::Deny)
    })
}

fn network_session(desired: &PlatformWebView) -> Result<NetworkSession> {
    let Some(storage_key) = desired.storage_key.as_ref() else {
        return Ok(NetworkSession::new_ephemeral());
    };
    PERSISTENT_SESSIONS.with(|sessions| {
        if let Some(session) = sessions.borrow().get(storage_key) {
            return Ok(session.clone());
        }
        let profile = webview_storage_dir(storage_key)?;
        let data = profile.join("data");
        let cache = profile.join("cache");
        std::fs::create_dir_all(&data).with_context(|| {
            format!("creating WebView profile data directory {}", data.display())
        })?;
        std::fs::create_dir_all(&cache).with_context(|| {
            format!(
                "creating WebView profile cache directory {}",
                cache.display()
            )
        })?;
        let data = path_utf8(&data)?;
        let cache = path_utf8(&cache)?;
        let session = NetworkSession::new(Some(data), Some(cache));
        sessions
            .borrow_mut()
            .insert(storage_key.clone(), session.clone());
        Ok(session)
    })
}

fn path_utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("WebView profile path is not UTF-8: {}", path.display()))
}

fn load_uri(
    webview: &WebView,
    url: &str,
    headers: Option<&http_client::http::HeaderMap>,
) -> Result<()> {
    let Some(headers) = headers else {
        webview.load_uri(url);
        return Ok(());
    };
    let request = URIRequest::new(url);
    let request_headers = request
        .http_headers()
        .context("WebKitGTK 6 URI request did not provide HTTP headers")?;
    for (name, value) in headers {
        request_headers.append(
            name.as_str(),
            value
                .to_str()
                .with_context(|| format!("WebView request header {name} is not text"))?,
        );
    }
    webview.load_request(&request);
    Ok(())
}

fn read_cookies(
    session: &NetworkSession,
    url: Option<SharedString>,
    callback: WebViewCookieCallback,
) {
    let Some(manager) = session.cookie_manager() else {
        glib::idle_add_local_once(move || {
            callback(Err("WebKitGTK 6 cookie manager unavailable".into()));
        });
        return;
    };
    let finish = move |result: std::result::Result<Vec<webkit6::soup::Cookie>, glib::Error>| {
        let result = result
            .map(|cookies| cookies.into_iter().map(cookie_from_soup).collect())
            .map_err(|error| error.to_string().into());
        catch_platform_callback("webview cookie read", (), || callback(result));
    };
    if let Some(url) = url {
        manager.cookies(&url, None::<&gio::Cancellable>, finish);
    } else {
        manager.all_cookies(None::<&gio::Cancellable>, finish);
    }
}

fn mutate_cookie(
    session: &NetworkSession,
    cookie: WebViewCookie,
    callback: WebViewCookieMutationCallback,
    delete: bool,
) {
    let Some(manager) = session.cookie_manager() else {
        glib::idle_add_local_once(move || {
            callback(Err("WebKitGTK 6 cookie manager unavailable".into()));
        });
        return;
    };
    let cookie = cookie_to_soup(cookie);
    let finish = move |result: std::result::Result<(), glib::Error>| {
        let result = result.map_err(|error| error.to_string().into());
        catch_platform_callback("webview cookie mutation", (), || callback(result));
    };
    if delete {
        manager.delete_cookie(&cookie, None::<&gio::Cancellable>, finish);
    } else {
        manager.add_cookie(&cookie, None::<&gio::Cancellable>, finish);
    }
}

fn cookie_from_soup(mut cookie: webkit6::soup::Cookie) -> WebViewCookie {
    WebViewCookie {
        name: cookie
            .name()
            .map(|value| value.to_string().into())
            .unwrap_or_default(),
        value: cookie
            .value()
            .map(|value| value.to_string().into())
            .unwrap_or_default(),
        domain: cookie.domain().map(|value| value.to_string().into()),
        path: cookie.path().map(|value| value.to_string().into()),
        secure: cookie.is_secure(),
        http_only: cookie.is_http_only(),
    }
}

fn cookie_to_soup(cookie: WebViewCookie) -> webkit6::soup::Cookie {
    let mut result = webkit6::soup::Cookie::new(
        &cookie.name,
        &cookie.value,
        cookie.domain.as_ref().map_or("", |value| value.as_ref()),
        cookie.path.as_ref().map_or("/", |value| value.as_ref()),
        -1,
    );
    result.set_secure(cookie.secure);
    result.set_http_only(cookie.http_only);
    result
}

fn permission_kind(request: &PermissionRequest) -> WebViewPermissionKind {
    if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
        if webkit6::functions::user_media_permission_is_for_display_device(media) {
            return WebViewPermissionKind::DisplayCapture;
        }
        if webkit6::functions::user_media_permission_is_for_audio_device(media) {
            return WebViewPermissionKind::Microphone;
        }
        if webkit6::functions::user_media_permission_is_for_video_device(media) {
            return WebViewPermissionKind::Camera;
        }
    }
    if request.is::<GeolocationPermissionRequest>() {
        WebViewPermissionKind::Geolocation
    } else if request.is::<NotificationPermissionRequest>() {
        WebViewPermissionKind::Notifications
    } else if request.is::<ClipboardPermissionRequest>() {
        WebViewPermissionKind::ClipboardRead
    } else if request.is::<MediaKeySystemPermissionRequest>() {
        WebViewPermissionKind::MediaKeySystemAccess
    } else if request.is::<PointerLockPermissionRequest>() {
        WebViewPermissionKind::PointerLock
    } else if request.is::<WebsiteDataAccessPermissionRequest>() {
        WebViewPermissionKind::Other
    } else {
        WebViewPermissionKind::Other
    }
}

fn webkit_bridge_script(storage_key: Option<&SharedString>, nonce: &str) -> String {
    let storage_key = storage_key
        .map(|key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string_literal(key)
            )
        })
        .unwrap_or_default();
    let nonce = json_string_literal(nonce);
    format!(
        "(() => {{ if (window.top !== window.self) return; {storage_key} const nonce = {nonce}; const send = message => {{ const body = typeof message === 'string' ? message : JSON.stringify(message); window.webkit.messageHandlers.kael.postMessage(JSON.stringify({{ __kaelIpcNonce: nonce, body }})); }}; window.external ??= {{}}; window.external.invoke = send; window.gpui ??= {{}}; window.gpui.postMessage = send; }})();"
    )
}

fn javascript_result(value: &webkit6::javascriptcore::Value) -> String {
    if value.is_string() {
        value.to_str().to_string()
    } else {
        value
            .to_json(0)
            .map_or_else(|| value.to_str().to_string(), |json| json.to_string())
    }
}

fn focus_parent(webview: &WebView) {
    if let Some(parent) = webview.parent() {
        parent.set_focusable(true);
        parent.grab_focus();
    }
}

fn sanitize_bounds(mut bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    bounds.origin.x.0 = finite(bounds.origin.x.0);
    bounds.origin.y.0 = finite(bounds.origin.y.0);
    bounds.size.width.0 = finite(bounds.size.width.0).max(0.0);
    bounds.size.height.0 = finite(bounds.size.height.0).max(0.0);
    bounds
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn finite_coordinate(value: f32) -> f64 {
    finite(value).clamp(i32::MIN as f32, i32::MAX as f32) as f64
}

fn finite_extent(value: f32) -> i32 {
    finite(value).clamp(0.0, i32::MAX as f32).round() as i32
}

fn gdk_rgba(color: Rgba) -> gtk4::gdk::RGBA {
    gtk4::gdk::RGBA::new(
        color.r.clamp(0.0, 1.0),
        color.g.clamp(0.0, 1.0),
        color.b.clamp(0.0, 1.0),
        color.a.clamp(0.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webkit_bridge_is_top_frame_only_and_nonce_authenticated() {
        let script = webkit_bridge_script(Some(&"profile".into()), "secret");
        assert!(script.contains("window.top !== window.self"));
        assert!(script.contains("messageHandlers.kael"));
        assert!(script.contains("__kaelIpcNonce"));
        assert!(script.contains("secret"));
        assert!(script.contains("GPUI_WEBVIEW_STORAGE_ID"));
    }

    #[test]
    fn webview_bounds_reject_non_finite_and_negative_extents() {
        let bounds = sanitize_bounds(Bounds {
            origin: crate::point(Pixels(f32::NAN), Pixels(f32::INFINITY)),
            size: crate::size(Pixels(-10.0), Pixels(f32::NAN)),
        });
        assert_eq!(bounds.origin.x.0, 0.0);
        assert_eq!(bounds.origin.y.0, 0.0);
        assert_eq!(bounds.size.width.0, 0.0);
        assert_eq!(bounds.size.height.0, 0.0);
    }
}
