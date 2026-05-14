use crate::{App, AsyncWindowContext, Bounds, Pixels, SharedString, Window};
use std::rc::Rc;

/// Controls whether a WebView navigation attempt should continue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationPolicy {
    /// Allow the navigation to proceed.
    Allow,
    /// Block the navigation.
    Deny,
}

pub(crate) type WebViewMessageHandler = Rc<dyn Fn(serde_json::Value, &mut Window, &mut App)>;

pub(crate) type WebViewNavigationHandler =
    Rc<dyn Fn(SharedString, &mut Window, &mut App) -> NavigationPolicy>;

#[derive(Clone)]
pub(crate) struct PlatformWebView {
    pub(crate) id: SharedString,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) url: SharedString,
    pub(crate) visible: bool,
    pub(crate) storage_key: Option<SharedString>,
    pub(crate) user_agent: Option<SharedString>,
    pub(crate) injected_css: Vec<SharedString>,
    pub(crate) injected_javascript: Vec<SharedString>,
    pub(crate) async_window: AsyncWindowContext,
    pub(crate) message_handler: Option<WebViewMessageHandler>,
    pub(crate) navigation_handler: Option<WebViewNavigationHandler>,
}

#[derive(Clone, Debug)]
pub(crate) enum PlatformWebViewCommand {
    Navigate {
        id: SharedString,
        url: SharedString,
    },
    EvaluateJavaScript {
        id: SharedString,
        script: SharedString,
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
}
