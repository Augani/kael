use crate::{App, AsyncWindowContext, Bounds, Pixels, Rgba, SharedString, Window};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Controls whether a WebView navigation attempt should continue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationPolicy {
    /// Allow the navigation to proceed.
    Allow,
    /// Block the navigation.
    Deny,
}

/// Controls how a WebView should handle `window.open` and target-blank requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebViewNewWindowPolicy {
    /// Block the requested new window.
    Deny,
    /// Navigate the existing WebView to the requested URL.
    NavigateCurrent,
    /// Let the platform WebView backend perform its default new-window behavior.
    Allow,
}

/// Controls whether a WebView download should proceed and where it should save.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebViewDownloadPolicy {
    /// Allow the backend to download to its default destination.
    Allow,
    /// Block the download.
    Deny,
    /// Allow the download and request an explicit absolute destination path.
    SaveTo(PathBuf),
}

/// Controls whether a WebView drag/drop event should reach browser defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebViewDragDropPolicy {
    /// Allow the embedded browser to handle the event normally.
    AllowBrowserDefault,
    /// Block the embedded browser and operating system default handling.
    BlockBrowserDefault,
}

impl WebViewDragDropPolicy {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn blocks_browser_default(self) -> bool {
        self == Self::BlockBrowserDefault
    }
}

/// Completion details for a WebView download.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebViewDownloadCompleted {
    /// The original URL requested by the WebView download.
    pub url: SharedString,
    /// The path reported by the backend, when available.
    pub path: Option<PathBuf>,
    /// Whether the backend reported the download as successful.
    pub success: bool,
}

/// Cookie data read from an embedded WebView.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebViewCookie {
    /// Cookie name.
    pub name: SharedString,
    /// Cookie value.
    pub value: SharedString,
    /// Cookie domain, when reported by the backend.
    pub domain: Option<SharedString>,
    /// Cookie path, when reported by the backend.
    pub path: Option<SharedString>,
    /// Whether the cookie is restricted to secure transports.
    pub secure: bool,
    /// Whether the cookie is hidden from page JavaScript.
    pub http_only: bool,
}

impl WebViewCookie {
    /// Create a cookie with a name and value.
    pub fn new(name: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: None,
            path: None,
            secure: false,
            http_only: false,
        }
    }

    /// Set the cookie domain.
    pub fn domain(mut self, domain: impl Into<SharedString>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// Set the cookie path.
    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Mark whether the cookie requires a secure transport.
    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    /// Mark whether the cookie should be hidden from page JavaScript.
    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }
}

/// Page loading lifecycle event for an embedded WebView.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebViewPageLoadEvent {
    /// The page has started loading.
    Started,
    /// The page content has finished loading.
    Finished,
}

/// A file drag/drop event delivered to a WebView island.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebViewDragDropEvent {
    /// A file drag entered the WebView.
    Enter {
        /// Paths currently being dragged.
        paths: Vec<PathBuf>,
        /// Position relative to the WebView top-left corner, in physical pixels.
        position: (i32, i32),
    },
    /// A file drag moved over the WebView.
    Over {
        /// Position relative to the WebView top-left corner, in physical pixels.
        position: (i32, i32),
    },
    /// Files were dropped onto the WebView.
    Drop {
        /// Dropped paths.
        paths: Vec<PathBuf>,
        /// Position relative to the WebView top-left corner, in physical pixels.
        position: (i32, i32),
    },
    /// A drag operation left or was cancelled.
    Leave,
}

pub(crate) type WebViewMessageHandler = Rc<dyn Fn(serde_json::Value, &mut Window, &mut App)>;

pub(crate) type WebViewNavigationHandler =
    Rc<dyn Fn(SharedString, &mut Window, &mut App) -> NavigationPolicy>;

pub(crate) type WebViewNewWindowHandler =
    Rc<dyn Fn(SharedString, &mut Window, &mut App) -> WebViewNewWindowPolicy>;

pub(crate) type WebViewDownloadStartedHandler =
    Rc<dyn Fn(SharedString, Option<PathBuf>, &mut Window, &mut App) -> WebViewDownloadPolicy>;

pub(crate) type WebViewDownloadCompletedHandler =
    Rc<dyn Fn(WebViewDownloadCompleted, &mut Window, &mut App)>;

pub(crate) type WebViewDocumentTitleChangedHandler =
    Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

pub(crate) type WebViewPageLoadHandler =
    Rc<dyn Fn(WebViewPageLoadEvent, SharedString, &mut Window, &mut App)>;

pub(crate) type WebViewDragDropHandler =
    Rc<dyn Fn(WebViewDragDropEvent, &mut Window, &mut App) -> WebViewDragDropPolicy>;

pub(crate) type WebViewCookieCallback =
    Rc<dyn Fn(Result<Vec<WebViewCookie>, SharedString>) + 'static>;

pub(crate) type WebViewCookieMutationCallback = Rc<dyn Fn(Result<(), SharedString>) + 'static>;

pub(crate) type WebViewUrlCallback = Rc<dyn Fn(Result<SharedString, SharedString>) + 'static>;

pub(crate) type WebViewDevToolsStateCallback = Rc<dyn Fn(Result<bool, SharedString>) + 'static>;

pub(crate) type WebViewJavaScriptResultCallback =
    Arc<dyn Fn(Result<SharedString, SharedString>) + Send + Sync + 'static>;

#[derive(Clone)]
pub(crate) struct PlatformWebView {
    pub(crate) id: SharedString,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) url: SharedString,
    pub(crate) html: Option<SharedString>,
    pub(crate) visible: bool,
    pub(crate) storage_key: Option<SharedString>,
    pub(crate) user_agent: Option<SharedString>,
    pub(crate) injected_css: Vec<SharedString>,
    pub(crate) injected_javascript: Vec<SharedString>,
    pub(crate) request_headers: Option<http_client::http::HeaderMap>,
    pub(crate) javascript_disabled: bool,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) general_autofill: Option<bool>,
    pub(crate) background_color: Option<Rgba>,
    pub(crate) devtools: bool,
    pub(crate) zoom_hotkeys_enabled: bool,
    pub(crate) media_autoplay: Option<bool>,
    pub(crate) focused: Option<bool>,
    pub(crate) clipboard_access: bool,
    pub(crate) async_window: AsyncWindowContext,
    pub(crate) message_handler: Option<WebViewMessageHandler>,
    pub(crate) navigation_handler: Option<WebViewNavigationHandler>,
    pub(crate) new_window_handler: Option<WebViewNewWindowHandler>,
    pub(crate) download_started_handler: Option<WebViewDownloadStartedHandler>,
    pub(crate) download_completed_handler: Option<WebViewDownloadCompletedHandler>,
    pub(crate) document_title_changed_handler: Option<WebViewDocumentTitleChangedHandler>,
    pub(crate) page_load_handler: Option<WebViewPageLoadHandler>,
    pub(crate) drag_drop_handler: Option<WebViewDragDropHandler>,
}

#[derive(Clone)]
pub(crate) enum PlatformWebViewCommand {
    Navigate {
        id: SharedString,
        url: SharedString,
    },
    NavigateWithHeaders {
        id: SharedString,
        url: SharedString,
        headers: http_client::http::HeaderMap,
    },
    LoadHtml {
        id: SharedString,
        html: SharedString,
    },
    EvaluateJavaScript {
        id: SharedString,
        script: SharedString,
    },
    EvaluateJavaScriptWithResult {
        id: SharedString,
        script: SharedString,
        callback: WebViewJavaScriptResultCallback,
    },
    PostMessage {
        id: SharedString,
        message: serde_json::Value,
    },
    Reload {
        id: SharedString,
    },
    GoBack {
        id: SharedString,
    },
    GoForward {
        id: SharedString,
    },
    OpenDevTools {
        id: SharedString,
    },
    CloseDevTools {
        id: SharedString,
    },
    IsDevToolsOpen {
        id: SharedString,
        callback: WebViewDevToolsStateCallback,
    },
    Print {
        id: SharedString,
    },
    SetZoomFactor {
        id: SharedString,
        factor: f64,
    },
    Focus {
        id: SharedString,
    },
    FocusParent {
        id: SharedString,
    },
    ClearBrowsingData {
        id: SharedString,
    },
    ReadUrl {
        id: SharedString,
        callback: WebViewUrlCallback,
    },
    ReadCookies {
        id: SharedString,
        url: Option<SharedString>,
        callback: WebViewCookieCallback,
    },
    SetCookie {
        id: SharedString,
        cookie: WebViewCookie,
        callback: WebViewCookieMutationCallback,
    },
    DeleteCookie {
        id: SharedString,
        cookie: WebViewCookie,
        callback: WebViewCookieMutationCallback,
    },
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn rgba_to_webview_color(color: Rgba) -> (u8, u8, u8, u8) {
    fn channel(value: f32) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    (
        channel(color.r),
        channel(color.g),
        channel(color.b),
        channel(color.a),
    )
}
