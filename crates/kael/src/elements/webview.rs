use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    NavigationPolicy, Pixels, SharedString, Style, StyleRefinement, Styled, Window,
    webview::{PlatformWebView, WebViewMessageHandler, WebViewNavigationHandler},
};
use refineable::Refineable;
use std::rc::Rc;

/// Creates a WebView element backed by the platform's native embedded web content view.
pub fn webview(id: impl Into<ElementId>, url: impl Into<SharedString>) -> WebView {
    WebView {
        element_id: id.into(),
        url: url.into(),
        style: StyleRefinement::default(),
        storage_key: None,
        user_agent: None,
        injected_css: Vec::new(),
        injected_javascript: Vec::new(),
        on_message: None,
        on_navigate: None,
    }
}

/// A native embedded WebView element.
pub struct WebView {
    element_id: ElementId,
    url: SharedString,
    style: StyleRefinement,
    storage_key: Option<SharedString>,
    user_agent: Option<SharedString>,
    injected_css: Vec<SharedString>,
    injected_javascript: Vec<SharedString>,
    on_message: Option<WebViewMessageHandler>,
    on_navigate: Option<WebViewNavigationHandler>,
}

impl WebView {
    /// Uses persistent storage when a key is supplied and ephemeral storage otherwise.
    pub fn storage_key(mut self, key: impl Into<SharedString>) -> Self {
        self.storage_key = Some(key.into());
        self
    }

    /// Overrides the user agent used by the platform WebView.
    pub fn user_agent(mut self, user_agent: impl Into<SharedString>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Injects CSS into each loaded page.
    pub fn inject_css(mut self, css: impl Into<SharedString>) -> Self {
        self.injected_css.push(css.into());
        self
    }

    /// Injects JavaScript into each loaded page.
    pub fn inject_javascript(mut self, javascript: impl Into<SharedString>) -> Self {
        self.injected_javascript.push(javascript.into());
        self
    }

    /// Registers a handler for messages posted from JavaScript.
    pub fn on_message(
        mut self,
        handler: impl Fn(serde_json::Value, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_message = Some(Rc::new(handler));
        self
    }

    /// Registers a handler that can allow or deny navigation attempts.
    pub fn on_navigate(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) -> NavigationPolicy + 'static,
    ) -> Self {
        self.on_navigate = Some(Rc::new(handler));
        self
    }
}

impl Element for WebView {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let webview = PlatformWebView {
            id: self.element_id.to_string().into(),
            bounds,
            url: self.url.clone(),
            visible: true,
            storage_key: self.storage_key.clone(),
            user_agent: self.user_agent.clone(),
            injected_css: self.injected_css.clone(),
            injected_javascript: self.injected_javascript.clone(),
            async_window: window.to_async(cx),
            message_handler: self.on_message.clone(),
            navigation_handler: self.on_navigate.clone(),
        };
        window.paint_webview(webview);
    }
}

impl IntoElement for WebView {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for WebView {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
