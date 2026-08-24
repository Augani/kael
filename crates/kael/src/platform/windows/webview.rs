use super::super::webview_common::{
    WryCustomProtocolRegistration, bridge_script, configure_wry_custom_protocols,
    create_web_context, css_script, decode_bridge_message, ipc_source_matches_top_level,
    main_frame_script, serialized_origin, to_wry_rect, warn_rejected_ipc_once, webview_command_id,
};
use super::WindowsWindowInner;
use crate::{
    AsyncWindowContext, Bounds, Pixels, SharedString, WebViewCookie, WebViewDownloadCompleted,
    WebViewDownloadPolicy, WebViewNewWindowPolicy,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand,
        WebViewDocumentTitleChangedHandler, WebViewDownloadCompletedHandler,
        WebViewDownloadStartedHandler, WebViewDragDropEvent, WebViewDragDropHandler,
        WebViewDragDropPolicy, WebViewNavigationHandler, WebViewNewWindowHandler,
        WebViewPageLoadEvent, WebViewPageLoadHandler, WebViewPermissionFrame,
        WebViewPermissionKind, rgba_to_webview_color,
    },
};
use anyhow::{Context as _, Result};
use parking_lot::RwLock;
use raw_window_handle as rwh;
use std::{
    cell::RefCell,
    collections::HashSet,
    num::NonZeroIsize,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};
use util::ResultExt;
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY,
        COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE, COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES,
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS, COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS,
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        COREWEBVIEW2_PERMISSION_STATE_DENY, ICoreWebView2,
        ICoreWebView2PermissionRequestedEventArgs3,
    },
    PermissionRequestedEventHandler, take_pwstr,
};
use windows_core_webview2::Interface as _;
use wry::{
    DragDropEvent as WryDragDropEvent, NewWindowResponse, PageLoadEvent, WebContext, WebView,
    WebViewBuilder, WebViewExtWindows,
};

/// A borrowed parent HWND for Wry's child-WebView construction.
///
/// `WindowsWindow` is the owning platform-window handle and its `Drop`
/// implementation destroys the HWND. Wrapping an `Rc<WindowsWindowInner>` in
/// a temporary `WindowsWindow` here would therefore close the application as
/// soon as `build_as_child` returned. This adapter exposes only the raw handle
/// and cannot acquire ownership of the parent window.
struct WindowsWebViewParent<'a>(&'a WindowsWindowInner);

impl rwh::HasWindowHandle for WindowsWebViewParent<'_> {
    fn window_handle(&self) -> std::result::Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let raw = rwh::Win32WindowHandle::new(unsafe {
            NonZeroIsize::new_unchecked(self.0.hwnd.0 as isize)
        })
        .into();
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw) })
    }
}

impl rwh::HasDisplayHandle for WindowsWebViewParent<'_> {
    fn display_handle(&self) -> std::result::Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(rwh::WindowsDisplayHandle::new().into()) })
    }
}

pub(crate) struct WindowsWebViewHost {
    desired: PlatformWebView,
    // Detach callbacks before Wry closes the underlying controller.
    _permission_registration: WindowsPermissionRegistration,
    webview: WebView,
    _protocol_registration: WryCustomProtocolRegistration,
    _context: Option<WebContext>,
    rendered_url: SharedString,
    rendered_html: Option<SharedString>,
    background_color: Option<crate::Rgba>,
    opacity: f32,
    live: Rc<RefCell<PlatformWebView>>,
    live_permission_handler: Arc<RwLock<Option<crate::webview::WebViewPermissionHandler>>>,
    live_top_level_origin: Arc<RwLock<Option<SharedString>>>,
}

struct WindowsPermissionRegistration {
    webview: ICoreWebView2,
    token: i64,
}

impl Drop for WindowsPermissionRegistration {
    fn drop(&mut self) {
        if let Err(error) = unsafe { self.webview.remove_PermissionRequested(self.token) } {
            log::debug!("failed to detach Windows WebView permission handler: {error}");
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
struct WindowsWebViewSignature {
    storage_key: Option<SharedString>,
    user_agent: Option<SharedString>,
    injected_css: Vec<SharedString>,
    injected_javascript: Vec<SharedString>,
    request_headers: Option<http_client::http::HeaderMap>,
    javascript_disabled: bool,
    general_autofill: Option<bool>,
    devtools: bool,
    zoom_hotkeys_enabled: bool,
    media_autoplay: Option<bool>,
    clipboard_access: bool,
    drag_drop_handler: bool,
    custom_protocol_schemes: Vec<SharedString>,
}

pub(crate) fn sync_webviews(window: &Rc<WindowsWindowInner>, webviews: &[PlatformWebView]) {
    let mut active_ids: HashSet<SharedString> = HashSet::default();

    for webview in webviews {
        let webview_id = webview.instance_id.clone();
        active_ids.insert(webview_id.clone());

        let (removed_host, scale_factor) = {
            let mut state = window.state.borrow_mut();
            let needs_recreate = state
                .webviews
                .get(&webview_id)
                .is_some_and(|host| host.needs_recreate(webview));
            let removed_host = needs_recreate
                .then(|| state.webviews.remove(&webview_id))
                .flatten();

            let scale_factor = state.scale_factor;
            if let Some(host) = state.webviews.get_mut(&webview_id) {
                host.update_desired(webview.clone(), scale_factor);
                (removed_host, None)
            } else {
                let should_spawn = !state.pending_webviews.contains_key(&webview_id);
                state
                    .pending_webviews
                    .insert(webview_id.clone(), webview.clone());
                (removed_host, should_spawn.then_some(scale_factor))
            }
        };
        // Dropping a WebView controller can dispatch native messages too.
        // Release it only after the platform-state borrow has ended.
        drop(removed_host);

        if let Some(scale_factor) = scale_factor {
            spawn_webview_creation(window.clone(), webview_id, webview.clone(), scale_factor);
        }
    }

    let stale_ids = window
        .state
        .borrow()
        .webviews
        .keys()
        .filter(|webview_id| !active_ids.contains(*webview_id))
        .cloned()
        .collect::<Vec<_>>();
    for webview_id in stale_ids {
        // As above, destroy native controllers without holding the state.
        let stale_host = window.state.borrow_mut().webviews.remove(&webview_id);
        drop(stale_host);
    }
    window
        .state
        .borrow_mut()
        .pending_webviews
        .retain(|webview_id, _| active_ids.contains(webview_id));
}

fn spawn_webview_creation(
    window: Rc<WindowsWindowInner>,
    webview_id: SharedString,
    desired: PlatformWebView,
    scale_factor: f32,
) {
    let executor = window.executor.clone();
    executor
        .spawn(async move {
            // WebView2 creates its controller synchronously and pumps Win32
            // messages while it waits. Run it after the current frame so those
            // messages can update the App and inspect the platform state.
            let result = WindowsWebViewHost::new(&window, desired, scale_factor);
            let (latest, latest_scale_factor) = {
                let mut state = window.state.borrow_mut();
                (
                    state.pending_webviews.remove(&webview_id),
                    state.scale_factor,
                )
            };
            let Some(latest) = latest else {
                return;
            };

            match result {
                Ok(mut host) if !host.needs_recreate(&latest) => {
                    host.update_desired(latest, latest_scale_factor);
                    window.state.borrow_mut().webviews.insert(webview_id, host);
                }
                Ok(host) => {
                    // Creation-only settings changed while WebView2 was
                    // starting. Drop this controller and schedule the latest
                    // configuration rather than exposing stale behavior.
                    drop(host);
                    window
                        .state
                        .borrow_mut()
                        .pending_webviews
                        .insert(webview_id.clone(), latest.clone());
                    spawn_webview_creation(window, webview_id, latest, latest_scale_factor);
                }
                Err(error) => {
                    log::error!("failed to create Windows WebView {}: {error:#}", latest.id);
                }
            }
        })
        .detach();
}

pub(crate) fn dispatch_webview_command(
    window: &Rc<WindowsWindowInner>,
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

impl WindowsWebViewHost {
    fn new(
        window: &Rc<WindowsWindowInner>,
        desired: PlatformWebView,
        scale_factor: f32,
    ) -> Result<Self> {
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
            live_top_level_origin.clone(),
        );
        let builder = configure_wry_custom_protocols(builder, &desired, &protocol_registration);

        let webview = builder
            .build_as_child(&WindowsWebViewParent(window.as_ref()))
            .context("building Windows child webview")?;
        webview.set_visible(desired.visible).log_err();
        let permission_registration =
            register_permission_handler(&webview, live_permission_handler.clone())?;

        let rendered_url = desired.url.clone();
        let rendered_html = desired.html.clone();
        let mut host = Self {
            background_color: desired.background_color,
            opacity: desired.opacity,
            live,
            live_permission_handler,
            live_top_level_origin,
            _permission_registration: permission_registration,
            desired,
            webview,
            _protocol_registration: protocol_registration,
            _context: context,
            rendered_url,
            rendered_html,
        };
        host.apply(scale_factor);
        Ok(host)
    }

    fn needs_recreate(&self, webview: &PlatformWebView) -> bool {
        WindowsWebViewSignature::from(&self.desired) != WindowsWebViewSignature::from(webview)
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
        self.webview
            .set_bounds(to_wry_rect(self.desired.bounds))
            .log_err();
        self.webview.set_visible(self.desired.visible).log_err();

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
            apply_webview_opacity(&self.webview, self.desired.opacity).log_err();
            self.opacity = self.desired.opacity;
        }
    }

    pub(crate) fn apply_command(&mut self, command: PlatformWebViewCommand) -> Result<()> {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.webview
                    .load_url(url.as_ref())
                    .context("navigating Windows WebView")?;
                *self.live_top_level_origin.write() = serialized_origin(&url);
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                self.webview
                    .load_url_with_headers(url.as_ref(), headers)
                    .context("navigating Windows WebView with headers")?;
                *self.live_top_level_origin.write() = serialized_origin(&url);
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.webview
                    .load_html(html.as_ref())
                    .context("loading HTML into Windows WebView")?;
                *self.live_top_level_origin.write() = None;
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.webview
                    .evaluate_script(script.as_ref())
                    .context("evaluating JavaScript in Windows WebView")?;
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
                let payload = serde_json::to_string(&message)
                    .context("serializing Windows WebView message")?;
                let script = format!(
                    "(() => {{ const payload = {payload}; if (window.dispatchEvent) {{ window.dispatchEvent(new MessageEvent('message', {{ data: payload }})); }} if (typeof window.onmessage === 'function') {{ window.onmessage({{ data: payload }}); }} }})();"
                );
                self.webview
                    .evaluate_script(&script)
                    .context("posting message into Windows WebView")?;
            }
            PlatformWebViewCommand::Reload { .. } => {
                self.webview.reload().context("reloading Windows WebView")?;
            }
            PlatformWebViewCommand::GoBack { .. } => {
                self.webview
                    .go_back()
                    .context("navigating Windows WebView backward")?;
            }
            PlatformWebViewCommand::GoForward { .. } => {
                self.webview
                    .go_forward()
                    .context("navigating Windows WebView forward")?;
            }
            PlatformWebViewCommand::OpenDevTools { .. } => {
                #[cfg(any(debug_assertions, feature = "devtools"))]
                self.webview.open_devtools();
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                anyhow::bail!(
                    "Windows WebView devtools require a debug build or `devtools` feature"
                );
            }
            PlatformWebViewCommand::CloseDevTools { .. } => {
                anyhow::bail!("closing Windows WebView devtools is not supported by WebView2");
            }
            PlatformWebViewCommand::IsDevToolsOpen { callback, .. } => {
                callback(Err(
                    "querying Windows WebView devtools state is not supported by WebView2".into(),
                ));
            }
            PlatformWebViewCommand::Print { .. } => {
                self.webview.print().context("printing Windows WebView")?;
            }
            PlatformWebViewCommand::SetZoomFactor { factor, .. } => {
                self.webview
                    .zoom(factor)
                    .context("setting Windows WebView zoom factor")?;
            }
            PlatformWebViewCommand::Focus { .. } => {
                self.webview.focus().context("focusing Windows WebView")?;
            }
            PlatformWebViewCommand::FocusParent { .. } => {
                self.webview
                    .focus_parent()
                    .context("focusing Windows WebView parent")?;
            }
            PlatformWebViewCommand::ClearBrowsingData { .. } => {
                self.webview
                    .clear_all_browsing_data()
                    .context("clearing Windows WebView browsing data")?;
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

impl From<&PlatformWebView> for WindowsWebViewSignature {
    fn from(webview: &PlatformWebView) -> Self {
        Self {
            storage_key: webview.storage_key.clone(),
            user_agent: webview.user_agent.clone(),
            injected_css: webview.injected_css.clone(),
            injected_javascript: webview.injected_javascript.clone(),
            request_headers: webview.request_headers.clone(),
            javascript_disabled: webview.javascript_disabled,
            general_autofill: webview.general_autofill,
            devtools: webview.devtools,
            zoom_hotkeys_enabled: webview.zoom_hotkeys_enabled,
            media_autoplay: webview.media_autoplay,
            clipboard_access: webview.clipboard_access,
            drag_drop_handler: webview.drag_drop_handler.is_some(),
            custom_protocol_schemes: webview.custom_protocol_schemes.clone(),
        }
    }
}

fn permission_kind_from_webview2(kind: COREWEBVIEW2_PERMISSION_KIND) -> WebViewPermissionKind {
    match kind {
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => WebViewPermissionKind::Microphone,
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => WebViewPermissionKind::Camera,
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => WebViewPermissionKind::Geolocation,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => WebViewPermissionKind::Notifications,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => WebViewPermissionKind::ClipboardRead,
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS => WebViewPermissionKind::LocalFonts,
        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS => WebViewPermissionKind::Sensors,
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES => WebViewPermissionKind::Midi,
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS => {
            WebViewPermissionKind::AutomaticDownloads
        }
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => WebViewPermissionKind::FileSystemAccess,
        COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY => WebViewPermissionKind::Autoplay,
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT => WebViewPermissionKind::WindowManagement,
        _ => WebViewPermissionKind::Other,
    }
}

fn register_permission_handler(
    webview: &WebView,
    live_handler: Arc<RwLock<Option<crate::webview::WebViewPermissionHandler>>>,
) -> Result<WindowsPermissionRegistration> {
    let core_webview = webview.webview();
    let persistence_api_reported = AtomicBool::new(false);
    let callback = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let Some(handler) = live_handler.read().clone() else {
            return Ok(());
        };

        let Ok(args_with_profile_policy) =
            args.cast::<ICoreWebView2PermissionRequestedEventArgs3>()
        else {
            // Without this interface WebView2 may persist an Allow (or the
            // user's answer to Default) and skip future Kael policy calls.
            // Fail closed so a stale profile decision cannot outlive a changed
            // handler or origin policy.
            if !persistence_api_reported.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!(
                    "Windows WebView2 runtime lacks transient permission decisions; denying native permissions"
                );
            }
            unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)? };
            return Ok(());
        };
        unsafe { args_with_profile_policy.SetSavesInProfile(false)? };

        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
        unsafe { args.PermissionKind(&mut kind)? };

        let uri = {
            let mut uri = Default::default();
            unsafe { args.Uri(&mut uri)? };
            take_pwstr(uri)
        };
        let user_gesture = {
            let mut is_user_initiated = Default::default();
            unsafe { args.IsUserInitiated(&mut is_user_initiated)? };
            is_user_initiated.as_bool()
        };
        let request = crate::WebViewNativePermissionRequest::with_requesting_origin(
            permission_kind_from_webview2(kind),
            serialized_origin(&uri),
            WebViewPermissionFrame::Unknown,
            Some(user_gesture),
        );
        let decision = super::catch_platform_callback(
            "webview native permission policy",
            crate::WebViewPermissionDecision::Deny,
            || handler(request),
        );

        match decision {
            crate::WebViewPermissionDecision::Allow => unsafe {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
            },
            crate::WebViewPermissionDecision::Deny => unsafe {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
            },
            crate::WebViewPermissionDecision::Default => {}
        }
        Ok(())
    }));

    let mut token = 0;
    unsafe { core_webview.add_PermissionRequested(&callback, &mut token) }
        .context("registering Windows WebView2 permission handler")?;
    Ok(WindowsPermissionRegistration {
        webview: core_webview,
        token,
    })
}

fn configure_webview_builder<'a>(
    mut builder: WebViewBuilder<'a>,
    desired: &PlatformWebView,
    bounds: Bounds<Pixels>,
    live: Rc<RefCell<PlatformWebView>>,
    live_top_level_origin: Arc<RwLock<Option<SharedString>>>,
) -> WebViewBuilder<'a> {
    let ipc_nonce = uuid::Uuid::new_v4().simple().to_string();
    if desired.storage_key.is_none() {
        builder = builder.with_incognito(true);
    }

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
    if let Some(enabled) = desired.general_autofill {
        builder = builder.with_general_autofill_enabled(enabled);
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
            warn_rejected_ipc_once(&rejected_ipc_reported, "Windows");
            return;
        }
        let Some(payload) = decode_bridge_message(request.body(), &ipc_nonce_for_handler) else {
            warn_rejected_ipc_once(&rejected_ipc_reported, "Windows");
            return;
        };
        super::catch_platform_callback("webview message", (), || {
            let _ = async_window.update(|window, cx| handler(payload, window, cx));
        });
    });

    let navigation_live = live.clone();
    let navigation_origin = live_top_level_origin.clone();
    builder = builder.with_navigation_handler(move |url| {
        let (handler, async_window) = {
            let live = navigation_live.borrow();
            (live.navigation_handler.clone(), live.async_window.clone())
        };
        let allowed = handle_navigation_request(&url, handler, async_window);
        if allowed {
            *navigation_origin.write() = serialized_origin(&url);
        }
        allowed
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
                    let _ = window.navigate_webview(webview_id.clone(), url.clone());
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
    if desired.opacity.clamp(0.0, 1.0) < 1.0 {
        builder = builder.with_initialization_script_for_main_only(
            webview_opacity_script(desired.opacity),
            true,
        );
    }
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

    if desired.drag_drop_handler.is_some() {
        let allow_default_reported = AtomicBool::new(false);
        builder = builder.with_drag_drop_handler(move |event| {
            let (handler, async_window) = {
                let live = live.borrow();
                (live.drag_drop_handler.clone(), live.async_window.clone())
            };
            if let Some(handler) = handler
                && dispatch_drag_drop_event(event, handler, async_window)
                    == WebViewDragDropPolicy::AllowBrowserDefault
                && !allow_default_reported.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                log::warn!(
                    "Windows WebView drag/drop interception replaces HTML drag/drop; AllowBrowserDefault cannot be honored"
                );
            }
            // Wry ignores this value on Windows.
            true
        });
    }

    builder
}

fn apply_webview_opacity(webview: &WebView, opacity: f32) -> Result<()> {
    // WebView2's HWND controller cannot be made layered or transparent without
    // breaking navigation on every maintained Windows release. Compositing the
    // document inside the controller preserves page lifecycle, input, and IPC.
    webview.evaluate_script(&webview_opacity_script(opacity))?;
    Ok(())
}

fn webview_opacity_script(opacity: f32) -> String {
    let opacity = opacity.clamp(0.0, 1.0);
    format!(
        r#"(() => {{
  const opacity = {opacity:.6};
  const key = Symbol.for("kael.nativeWebViewOpacity");
  const apply = () => {{
    const root = document.documentElement;
    if (!root) return;
    const previous = globalThis[key];
    if (previous?.animation) previous.animation.cancel();
    if (previous?.fallback) {{
      root.style.setProperty(
        "opacity",
        previous.originalValue,
        previous.originalPriority,
      );
    }}
    if (opacity >= 1) {{
      globalThis[key] = undefined;
      return;
    }}
    if (typeof root.animate === "function") {{
      const animation = root.animate(
        [{{ opacity }}, {{ opacity }}],
        {{ duration: 1, fill: "both" }},
      );
      globalThis[key] = {{ animation }};
    }} else {{
      const originalValue = root.style.getPropertyValue("opacity");
      const originalPriority = root.style.getPropertyPriority("opacity");
      root.style.setProperty("opacity", String(opacity), "important");
      globalThis[key] = {{ fallback: true, originalValue, originalPriority }};
    }}
  }};
  if (document.documentElement) apply();
  else addEventListener("DOMContentLoaded", apply, {{ once: true }});
}})();"#
    )
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

    if handle_navigation_request(url, navigation_handler, async_window) {
        WebViewNewWindowPolicy::NavigateCurrent
    } else {
        WebViewNewWindowPolicy::Deny
    }
}

fn handle_navigation_request(
    url: &str,
    navigation_handler: Option<WebViewNavigationHandler>,
    async_window: AsyncWindowContext,
) -> bool {
    let url = url.to_string();
    let allow = if let Some(handler) = navigation_handler {
        let mut async_window = async_window.clone();
        super::catch_platform_callback("webview navigation policy", NavigationPolicy::Deny, || {
            async_window
                .update(|window, cx| handler(url.clone().into(), window, cx))
                .unwrap_or(NavigationPolicy::Deny)
        }) == NavigationPolicy::Allow
    } else {
        true
    };

    if allow && is_external_scheme(&url) {
        let mut async_window = async_window;
        let _ = async_window.update(|_, cx| {
            cx.open_url(&url).log_err();
        });
        return false;
    }

    allow
}

fn is_external_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };

    !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "file" | "about" | "data" | "javascript" | "blob"
    )
}
