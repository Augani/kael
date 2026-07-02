use super::super::webview_common::{
    bridge_script, create_web_context, css_script, to_wry_rect, webview_command_id,
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
use std::{collections::HashSet, rc::Rc};
use util::ResultExt;
use wry::{
    DragDropEvent as WryDragDropEvent, NewWindowResponse, PageLoadEvent, WebContext, WebView,
    WebViewBuilder,
};

pub(crate) struct WindowsWebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    _context: Option<WebContext>,
    current_url: SharedString,
    current_html: Option<SharedString>,
    background_color: Option<crate::Rgba>,
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
        let webview_id = webview.id.clone();
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
    let Some(host) = state.webviews.get_mut(&webview_id) else {
        anyhow::bail!("unknown webview: {}", webview_id);
    };
    host.apply_command(command);
    Ok(())
}

impl WindowsWebViewHost {
    fn new(
        window: &Rc<WindowsWindowInner>,
        desired: PlatformWebView,
        scale_factor: f32,
    ) -> Result<Self> {
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
            .build_as_child(&WindowsWindow(window.clone()))
            .context("building Windows child webview")?;
        webview.set_visible(desired.visible).log_err();

        let current_url = desired.url.clone();
        let current_html = desired.html.clone();
        let mut host = Self {
            background_color: desired.background_color,
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
            || !same_optional_drag_drop_handler(
                &self.desired.drag_drop_handler,
                &webview.drag_drop_handler,
            )
    }

    fn update_desired(&mut self, desired: PlatformWebView, scale_factor: f32) {
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
    }

    pub(crate) fn apply_command(&mut self, command: PlatformWebViewCommand) {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.webview.load_url(url.as_ref()).log_err();
                self.current_url = url;
                self.current_html = None;
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                self.webview
                    .load_url_with_headers(url.as_ref(), headers)
                    .log_err();
                self.current_url = url;
                self.current_html = None;
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.webview.load_html(html.as_ref()).log_err();
                self.current_url = SharedString::default();
                self.current_html = Some(html);
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.webview.evaluate_script(script.as_ref()).log_err();
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
                    callback(Err(error.to_string().into()));
                }
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
            PlatformWebViewCommand::OpenDevTools { .. } => {
                #[cfg(debug_assertions)]
                self.webview.open_devtools();
                #[cfg(not(debug_assertions))]
                log::warn!("WebView devtools require a debug build or backend devtools support");
            }
            PlatformWebViewCommand::CloseDevTools { .. } => {
                #[cfg(debug_assertions)]
                self.webview.close_devtools();
                #[cfg(not(debug_assertions))]
                log::warn!("WebView devtools require a debug build or backend devtools support");
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
                self.webview.print().log_err();
            }
            PlatformWebViewCommand::SetZoomFactor { factor, .. } => {
                self.webview.zoom(factor).log_err();
            }
            PlatformWebViewCommand::Focus { .. } => {
                self.webview.focus().log_err();
            }
            PlatformWebViewCommand::FocusParent { .. } => {
                self.webview.focus_parent().log_err();
            }
            PlatformWebViewCommand::ClearBrowsingData { .. } => {
                self.webview.clear_all_browsing_data().log_err();
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
    .map_err(|error| error.to_string().into())?;

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
) -> WebViewBuilder<'a> {
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

    let navigation_handler = desired.navigation_handler.clone();
    let navigation_async_window = desired.async_window.clone();
    builder = builder.with_navigation_handler(move |url| {
        handle_navigation_request(
            &url,
            navigation_handler.clone(),
            navigation_async_window.clone(),
        )
    });

    let new_window_async_window = desired.async_window.clone();
    let new_window_id = desired.id.clone();
    let new_window_navigation_handler = desired.navigation_handler.clone();
    let new_window_handler = desired.new_window_handler.clone();
    builder = builder.with_new_window_req_handler(move |url, _features| {
        match resolve_new_window_policy(
            &url,
            new_window_handler.clone(),
            new_window_navigation_handler.clone(),
            new_window_async_window.clone(),
        ) {
            WebViewNewWindowPolicy::Deny => {}
            WebViewNewWindowPolicy::NavigateCurrent => {
                let mut async_window = new_window_async_window.clone();
                let webview_id = new_window_id.clone();
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

    if desired.download_started_handler.is_some() {
        let download_async_window = desired.async_window.clone();
        let download_started_handler = desired.download_started_handler.clone();
        builder = builder.with_download_started_handler(move |url, path| {
            resolve_download_started(
                url,
                path,
                download_started_handler.clone(),
                download_async_window.clone(),
            )
        });
    }

    if let Some(download_completed_handler) = desired.download_completed_handler.clone() {
        let download_async_window = desired.async_window.clone();
        builder = builder.with_download_completed_handler(move |url, path, success| {
            dispatch_download_completed(
                url,
                path,
                success,
                download_completed_handler.clone(),
                download_async_window.clone(),
            );
        });
    }

    if let Some(title_changed_handler) = desired.document_title_changed_handler.clone() {
        let title_async_window = desired.async_window.clone();
        builder = builder.with_document_title_changed_handler(move |title| {
            dispatch_document_title_changed(
                title,
                title_changed_handler.clone(),
                title_async_window.clone(),
            );
        });
    }

    if let Some(page_load_handler) = desired.page_load_handler.clone() {
        let page_load_async_window = desired.async_window.clone();
        builder = builder.with_on_page_load_handler(move |event, url| {
            dispatch_page_load(
                event,
                url,
                page_load_handler.clone(),
                page_load_async_window.clone(),
            );
        });
    }

    if let Some(drag_drop_handler) = desired.drag_drop_handler.clone() {
        let drag_drop_async_window = desired.async_window.clone();
        builder = builder.with_drag_drop_handler(move |event| {
            dispatch_drag_drop_event(
                event,
                drag_drop_handler.clone(),
                drag_drop_async_window.clone(),
            )
            .blocks_browser_default()
        });
    }

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
    async_window
        .update(|window, cx| handler(event, window, cx))
        .unwrap_or(WebViewDragDropPolicy::BlockBrowserDefault)
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
    let _ = async_window.update(|window, cx| handler(event, url.into(), window, cx));
}

fn dispatch_document_title_changed(
    title: String,
    handler: WebViewDocumentTitleChangedHandler,
    async_window: AsyncWindowContext,
) {
    let mut async_window = async_window.clone();
    let _ = async_window.update(|window, cx| handler(title.into(), window, cx));
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
    match async_window
        .update(|window, cx| handler(url.into(), suggested_path, window, cx))
        .unwrap_or(WebViewDownloadPolicy::Deny)
    {
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
    let _ = async_window.update(|window, cx| handler(event, window, cx));
}

fn resolve_new_window_policy(
    url: &str,
    new_window_handler: Option<WebViewNewWindowHandler>,
    navigation_handler: Option<WebViewNavigationHandler>,
    async_window: AsyncWindowContext,
) -> WebViewNewWindowPolicy {
    if let Some(handler) = new_window_handler {
        let mut async_window = async_window.clone();
        return async_window
            .update(|window, cx| handler(url.to_string().into(), window, cx))
            .unwrap_or(WebViewNewWindowPolicy::Deny);
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
        async_window
            .update(|window, cx| handler(url.clone().into(), window, cx))
            .unwrap_or(NavigationPolicy::Deny)
            == NavigationPolicy::Allow
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

fn same_optional_drag_drop_handler(
    left: &Option<WebViewDragDropHandler>,
    right: &Option<WebViewDragDropHandler>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}
