use super::super::webview_common::{
    bridge_script, create_web_context, css_script, permission_kind_from_wry,
    permission_response_to_wry, to_wry_rect, webview_command_id,
};
use super::{WindowsWindow, WindowsWindowInner};
use crate::{
    AsyncWindowContext, Bounds, Pixels, SharedString, WebViewCookie, WebViewDownloadCompleted,
    WebViewDownloadPolicy, WebViewNewWindowPolicy,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand,
        WebViewDocumentTitleChangedHandler, WebViewDownloadCompletedHandler,
        WebViewDownloadStartedHandler, WebViewDragDropEvent, WebViewDragDropHandler,
        WebViewDragDropPolicy, WebViewNavigationHandler, WebViewNewWindowHandler,
        WebViewPageLoadEvent, WebViewPageLoadHandler, rgba_to_webview_color,
    },
};
use anyhow::{Context as _, Result};
use parking_lot::RwLock;
use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};
use util::ResultExt;
use windows::Win32::{
    Foundation::{COLORREF, HWND},
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos, WS_EX_LAYERED,
    },
};
use wry::{
    DragDropEvent as WryDragDropEvent, NewWindowResponse, PageLoadEvent, WebContext, WebView,
    WebViewBuilder, WebViewExtWindows,
};

pub(crate) struct WindowsWebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    _context: Option<WebContext>,
    current_url: SharedString,
    current_html: Option<SharedString>,
    background_color: Option<crate::Rgba>,
    opacity: f32,
    live: Rc<RefCell<PlatformWebView>>,
    live_permission_handler: Arc<RwLock<Option<crate::webview::WebViewPermissionHandler>>>,
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
    focused: Option<bool>,
    clipboard_access: bool,
}

pub(crate) fn sync_webviews(window: &Rc<WindowsWindowInner>, webviews: &[PlatformWebView]) {
    let mut active_ids: HashSet<SharedString> = HashSet::default();
    let mut state = window.state.borrow_mut();

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

        let scale_factor = state.scale_factor;
        if let Some(host) = state.webviews.get_mut(&webview_id) {
            host.update_desired(webview.clone(), scale_factor);
        } else {
            match WindowsWebViewHost::new(window, webview.clone(), scale_factor) {
                Ok(host) => {
                    state.webviews.insert(webview_id, host);
                }
                Err(error) => {
                    log::error!("failed to create Windows WebView {}: {error:#}", webview.id);
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
            .build_as_child(&WindowsWindow(window.clone()))
            .context("building Windows child webview")?;
        webview.set_visible(desired.visible).log_err();

        let current_url = desired.url.clone();
        let current_html = desired.html.clone();
        let mut host = Self {
            background_color: desired.background_color,
            opacity: -1.0,
            live,
            live_permission_handler,
            desired,
            webview,
            _context: context,
            current_url,
            current_html,
        };
        host.apply(scale_factor);
        Ok(host)
    }

    fn needs_recreate(&self, webview: &PlatformWebView) -> bool {
        WindowsWebViewSignature::from(&self.desired) != WindowsWebViewSignature::from(webview)
    }

    fn update_desired(&mut self, desired: PlatformWebView, scale_factor: f32) {
        *self.live.borrow_mut() = desired.clone();
        *self.live_permission_handler.write() = desired.permission_handler.clone();
        self.desired = desired;
        self.apply(scale_factor);
    }

    fn apply(&mut self, _scale_factor: f32) {
        self.webview
            .set_bounds(to_wry_rect(self.desired.bounds))
            .log_err();
        self.webview.set_visible(self.desired.visible).log_err();

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
                self.current_url = url;
                self.current_html = None;
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                self.webview
                    .load_url_with_headers(url.as_ref(), headers)
                    .context("navigating Windows WebView with headers")?;
                self.current_url = url;
                self.current_html = None;
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.webview
                    .load_html(html.as_ref())
                    .context("loading HTML into Windows WebView")?;
                self.current_url = SharedString::default();
                self.current_html = Some(html);
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
                    .evaluate_script("history.back()")
                    .context("navigating Windows WebView backward")?;
            }
            PlatformWebViewCommand::GoForward { .. } => {
                self.webview
                    .evaluate_script("history.forward()")
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
                #[cfg(any(debug_assertions, feature = "devtools"))]
                self.webview.close_devtools();
                #[cfg(not(any(debug_assertions, feature = "devtools")))]
                anyhow::bail!(
                    "Windows WebView devtools require a debug build or `devtools` feature"
                );
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
            focused: webview.focused,
            clipboard_access: webview.clipboard_access,
        }
    }
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
    if let Some(enabled) = desired.general_autofill {
        builder = builder.with_general_autofill_enabled(enabled);
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
        let (handler, async_window) = {
            let live = navigation_live.borrow();
            (live.navigation_handler.clone(), live.async_window.clone())
        };
        handle_navigation_request(&url, handler, async_window)
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

fn apply_webview_opacity(webview: &WebView, opacity: f32) -> Result<()> {
    let wry_hwnd = webview.hwnd();
    let hwnd = HWND(wry_hwnd.0);
    let alpha = (opacity.clamp(0.0, 1.0) * u8::MAX as f32).round() as u8;

    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if ex_style & WS_EX_LAYERED.0 as i32 == 0 {
            let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            )?;
        }
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)?;
    }
    Ok(())
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
