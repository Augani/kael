use super::super::webview_common::{
    bridge_script, create_web_context, css_script, to_wry_rect, webview_command_id,
};
use super::{WindowsWindow, WindowsWindowInner};
use crate::{
    AsyncWindowContext, Bounds, Pixels, SharedString,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand, WebViewNavigationHandler,
    },
};
use anyhow::{Context as _, Result};
use std::{collections::HashSet, rc::Rc};
use util::ResultExt;
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

pub(crate) struct WindowsWebViewHost {
    desired: PlatformWebView,
    webview: WebView,
    _context: Option<WebContext>,
    current_url: SharedString,
}

#[derive(Clone, Eq, PartialEq)]
struct WindowsWebViewSignature {
    storage_key: Option<SharedString>,
    user_agent: Option<SharedString>,
    injected_css: Vec<SharedString>,
    injected_javascript: Vec<SharedString>,
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
        let mut host = Self {
            desired,
            webview,
            _context: context,
            current_url,
        };
        host.apply(scale_factor);
        Ok(host)
    }

    fn needs_recreate(&self, webview: &PlatformWebView) -> bool {
        WindowsWebViewSignature::from(&self.desired) != WindowsWebViewSignature::from(webview)
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
            self.webview.load_url(self.desired.url.as_ref()).log_err();
            self.current_url = self.desired.url.clone();
        }
    }

    pub(crate) fn apply_command(&mut self, command: PlatformWebViewCommand) {
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

impl From<&PlatformWebView> for WindowsWebViewSignature {
    fn from(webview: &PlatformWebView) -> Self {
        Self {
            storage_key: webview.storage_key.clone(),
            user_agent: webview.user_agent.clone(),
            injected_css: webview.injected_css.clone(),
            injected_javascript: webview.injected_javascript.clone(),
        }
    }
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
    builder = builder.with_new_window_req_handler(move |url, _features| {
        if handle_navigation_request(
            &url,
            new_window_navigation_handler.clone(),
            new_window_async_window.clone(),
        ) {
            let mut async_window = new_window_async_window.clone();
            let webview_id = new_window_id.clone();
            let _ = async_window.update(|window, _| {
                let _ = window.navigate_webview(webview_id.clone(), url.clone());
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
