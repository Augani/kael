use super::window_layer_id;
use crate::{
    BrowserWebViewLoading, BrowserWebViewSandbox, NavigationPolicy, SharedString, WebViewCookie,
    WebViewPageLoadEvent, WebViewPermissionKind,
    webview::{PlatformWebView, PlatformWebViewCommand},
};
use anyhow::{Context as _, Result, anyhow, bail, ensure};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlCanvasElement, HtmlElement, HtmlIFrameElement, MessageEvent};

const IPC_INSTANCE_FIELD: &str = "__kaelWebViewInstance";
const IPC_NONCE_FIELD: &str = "__kaelIpcNonce";
const IPC_BODY_FIELD: &str = "body";

/// DOM iframe overlays hosted above Kael's WebGL canvas.
pub(super) struct BrowserWebViewManager {
    layer: HtmlElement,
    hosts: HashMap<SharedString, BrowserWebViewHost>,
    pending_commands: HashMap<SharedString, Vec<PlatformWebViewCommand>>,
    parent_visible: Cell<bool>,
}

struct BrowserWebViewHost {
    iframe: HtmlIFrameElement,
    live: Rc<RefCell<PlatformWebView>>,
    declarative_source: RefCell<SourceSignature>,
    current_source: Rc<RefCell<SourceSignature>>,
    expected_origin: Rc<RefCell<Option<String>>>,
    navigation_generation: Rc<Cell<u64>>,
    alive: Rc<Cell<bool>>,
    nonce: String,
    load_callback: Closure<dyn FnMut(Event)>,
    warned_headers: Cell<bool>,
    warned_profile: Cell<bool>,
    warned_user_agent: Cell<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceSignature {
    Url(SharedString),
    Html(SharedString),
    Empty,
}

impl SourceSignature {
    fn from_webview(webview: &PlatformWebView) -> Self {
        if !webview.url.is_empty() {
            Self::Url(webview.url.clone())
        } else if let Some(html) = webview.html.clone() {
            Self::Html(html)
        } else {
            Self::Empty
        }
    }

    fn callback_url(&self) -> Result<SharedString> {
        Ok(match self {
            Self::Url(url) => resolve_url(url.as_ref())?.0.into(),
            Self::Html(_) => "about:srcdoc".into(),
            Self::Empty => "about:blank".into(),
        })
    }
}

impl BrowserWebViewManager {
    pub(super) fn new(canvas: &HtmlCanvasElement) -> Result<Self> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser WebView hosting requires a Document")?;
        let body = document
            .body()
            .context("browser WebView hosting requires a document body")?;
        let layer = document
            .create_element("div")
            .map_err(js_error)?
            .dyn_into::<HtmlElement>()
            .map_err(|_| anyhow!("browser WebView layer was not an HtmlElement"))?;
        layer.set_id(&window_layer_id(canvas, "kael-webview-layer"));
        layer
            .set_attribute("data-kael-webview-layer", "true")
            .map_err(js_error)?;
        let style = layer.style();
        style.set_property("position", "fixed").map_err(js_error)?;
        style.set_property("inset", "0").map_err(js_error)?;
        style
            .set_property("pointer-events", "none")
            .map_err(js_error)?;
        style
            .set_property("overflow", "visible")
            .map_err(js_error)?;
        style
            .set_property("z-index", "2147483646")
            .map_err(js_error)?;
        body.append_child(&layer).map_err(js_error)?;
        set_document_webview_count(0);
        initialize_document_webview_message_count();
        Ok(Self {
            layer,
            hosts: HashMap::new(),
            pending_commands: HashMap::new(),
            parent_visible: Cell::new(true),
        })
    }

    pub(super) fn sync(
        &mut self,
        canvas: &HtmlCanvasElement,
        webviews: &[PlatformWebView],
    ) -> Result<()> {
        let mut active = HashSet::new();
        let mut command_counts = HashMap::<SharedString, usize>::new();
        for webview in webviews {
            *command_counts.entry(webview.id.clone()).or_default() += 1;
        }

        for webview in webviews {
            let instance_id = webview.instance_id.clone();
            active.insert(instance_id.clone());
            let needs_recreate = self.hosts.get(&instance_id).is_some_and(|host| {
                let live = host.live.borrow();
                live.javascript_disabled != webview.javascript_disabled
                    || live.browser_policy != webview.browser_policy
            });
            if needs_recreate {
                self.hosts.remove(&instance_id);
            }

            if let Some(host) = self.hosts.get(&instance_id) {
                host.sync(webview.clone(), canvas, self.parent_visible.get())?;
            } else {
                let host = BrowserWebViewHost::new(
                    &self.layer,
                    webview.clone(),
                    canvas,
                    self.parent_visible.get(),
                )?;
                if command_counts.get(&webview.id) == Some(&1)
                    && let Some(commands) = self.pending_commands.remove(&webview.id)
                {
                    for command in commands {
                        host.apply_command(canvas, command)?;
                    }
                }
                self.hosts.insert(instance_id, host);
            }
        }

        self.hosts
            .retain(|instance_id, _| active.contains(instance_id));
        // Moving an existing iframe with `appendChild` destroys/reloads its browsing context in
        // browsers. Keep hosts attached and express Kael's presentation order with z-index.
        for (order, webview) in webviews.iter().enumerate() {
            if let Some(host) = self.hosts.get(&webview.instance_id) {
                host.iframe
                    .style()
                    .set_property("z-index", &order.to_string())
                    .map_err(js_error)?;
            }
        }
        set_document_webview_count(self.hosts.len());
        self.reposition(canvas);
        Ok(())
    }

    pub(super) fn dispatch(
        &mut self,
        canvas: &HtmlCanvasElement,
        command: PlatformWebViewCommand,
    ) -> Result<()> {
        let command_id = command_id(&command);
        let mut matches = self
            .hosts
            .values()
            .filter(|host| host.live.borrow().id == command_id);
        if let Some(host) = matches.next() {
            ensure!(
                matches.next().is_none(),
                "ambiguous webview id `{command_id}`; WebView command ids must be unique within a window"
            );
            host.apply_command(canvas, command)
        } else {
            self.pending_commands
                .entry(command_id)
                .or_default()
                .push(command);
            Ok(())
        }
    }

    pub(super) fn handle_message(&self, canvas: &HtmlCanvasElement, event: &MessageEvent) {
        let Some(envelope) = message_json(event) else {
            return;
        };
        let Some(instance_id) = envelope
            .get(IPC_INSTANCE_FIELD)
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Some(host) = self.hosts.get(instance_id) else {
            return;
        };
        if !host.message_source_matches(event)
            || envelope
                .get(IPC_NONCE_FIELD)
                .and_then(serde_json::Value::as_str)
                != Some(host.nonce.as_str())
        {
            return;
        }
        let Some(body) = envelope
            .get(IPC_BODY_FIELD)
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let payload = serde_json::from_str(body)
            .unwrap_or_else(|_| serde_json::Value::String(body.to_owned()));
        set_document_webview_message_received(canvas);
        dispatch_message(&host.live, payload);
    }

    pub(super) fn reposition(&self, canvas: &HtmlCanvasElement) {
        let canvas_rect = canvas.get_bounding_client_rect();
        for host in self.hosts.values() {
            host.position(&canvas_rect, self.parent_visible.get());
        }
    }

    pub(super) fn set_parent_visible(&self, visible: bool) {
        self.parent_visible.set(visible);
        let _ = self
            .layer
            .style()
            .set_property("display", if visible { "block" } else { "none" });
    }
}

impl Drop for BrowserWebViewManager {
    fn drop(&mut self) {
        self.hosts.clear();
        self.layer.remove();
        set_document_webview_count(0);
    }
}

impl BrowserWebViewHost {
    fn new(
        layer: &HtmlElement,
        desired: PlatformWebView,
        canvas: &HtmlCanvasElement,
        parent_visible: bool,
    ) -> Result<Self> {
        validate_source_policy(&desired, &SourceSignature::from_webview(&desired))?;
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser WebView hosting requires a Document")?;
        let iframe = document
            .create_element("iframe")
            .map_err(js_error)?
            .dyn_into::<HtmlIFrameElement>()
            .map_err(|_| anyhow!("browser did not create an iframe element"))?;
        iframe
            .set_attribute("data-kael-webview", desired.instance_id.as_ref())
            .map_err(js_error)?;
        iframe
            .set_attribute("title", desired.id.as_ref())
            .map_err(js_error)?;
        iframe.set_allow_fullscreen(true);
        let style = iframe.style();
        style
            .set_property("position", "absolute")
            .map_err(js_error)?;
        style.set_property("border", "0").map_err(js_error)?;
        style
            .set_property("pointer-events", "auto")
            .map_err(js_error)?;
        style.set_property("display", "none").map_err(js_error)?;

        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let live = Rc::new(RefCell::new(desired.clone()));
        let initial_source = SourceSignature::from_webview(&desired);
        let current_source = Rc::new(RefCell::new(SourceSignature::Empty));
        let expected_origin = Rc::new(RefCell::new(None));
        let navigation_generation = Rc::new(Cell::new(0));
        let alive = Rc::new(Cell::new(true));
        let iframe_for_load = iframe.clone();
        let live_for_load = live.clone();
        let current_source_for_load = current_source.clone();
        let expected_origin_for_load = expected_origin.clone();
        let nonce_for_load = nonce.clone();
        let load_callback = Closure::wrap(Box::new(move |_event: Event| {
            on_frame_loaded(
                &iframe_for_load,
                &live_for_load,
                &current_source_for_load,
                &expected_origin_for_load,
                &nonce_for_load,
            );
        }) as Box<dyn FnMut(Event)>);
        iframe
            .add_event_listener_with_callback("load", load_callback.as_ref().unchecked_ref())
            .map_err(js_error)?;
        apply_frame_policy(&iframe, &desired)?;
        layer.append_child(&iframe).map_err(js_error)?;

        let host = Self {
            iframe,
            live,
            declarative_source: RefCell::new(initial_source.clone()),
            current_source,
            expected_origin,
            navigation_generation,
            alive,
            nonce,
            load_callback,
            warned_headers: Cell::new(false),
            warned_profile: Cell::new(false),
            warned_user_agent: Cell::new(false),
        };
        host.warn_unsupported_options();
        host.load_source(initial_source)?;
        host.position(&canvas.get_bounding_client_rect(), parent_visible);
        if desired.focused == Some(true) {
            let _ = host.iframe.focus();
        }
        Ok(host)
    }

    fn sync(
        &self,
        desired: PlatformWebView,
        canvas: &HtmlCanvasElement,
        parent_visible: bool,
    ) -> Result<()> {
        let incoming_source = SourceSignature::from_webview(&desired);
        validate_source_policy(&desired, &incoming_source)?;
        let source_changed = *self.declarative_source.borrow() != incoming_source;
        let (focused_changed, frame_policy_changed) = {
            let live = self.live.borrow();
            (
                live.focused != desired.focused,
                live.media_autoplay != desired.media_autoplay
                    || live.clipboard_access != desired.clipboard_access,
            )
        };
        *self.live.borrow_mut() = desired;
        self.warn_unsupported_options();
        if frame_policy_changed {
            apply_frame_policy(&self.iframe, &self.live.borrow())?;
        }
        self.apply_style(parent_visible)?;
        if source_changed {
            *self.declarative_source.borrow_mut() = incoming_source.clone();
            self.load_source(incoming_source)?;
        }
        self.position(&canvas.get_bounding_client_rect(), parent_visible);
        if focused_changed {
            match self.live.borrow().focused {
                Some(true) => self.iframe.focus().map_err(js_error)?,
                Some(false) => self.iframe.blur().map_err(js_error)?,
                None => {}
            }
        }
        Ok(())
    }

    fn warn_unsupported_options(&self) {
        let live = self.live.borrow();
        if live.request_headers.is_some() && !self.warned_headers.replace(true) {
            log::warn!(
                "browser iframe WebView `{}` cannot attach custom request headers",
                live.id
            );
        }
        if live.storage_key.is_some() && !self.warned_profile.replace(true) {
            log::warn!(
                "browser iframe WebView `{}` cannot create a storage-key-isolated profile; storage remains origin-owned",
                live.id
            );
        }
        if live.user_agent.is_some() && !self.warned_user_agent.replace(true) {
            log::warn!(
                "browser iframe WebView `{}` cannot override the browser user agent",
                live.id
            );
        }
    }

    fn load_source(&self, source: SourceSignature) -> Result<()> {
        validate_source_policy(&self.live.borrow(), &source)?;
        let generation = self.navigation_generation.get().wrapping_add(1);
        self.navigation_generation.set(generation);

        if matches!(&source, SourceSignature::Url(_))
            && self.live.borrow().navigation_handler.is_some()
        {
            let iframe = self.iframe.clone();
            let live = self.live.clone();
            let current_source = self.current_source.clone();
            let expected_origin = self.expected_origin.clone();
            let navigation_generation = self.navigation_generation.clone();
            let alive = self.alive.clone();
            let nonce = self.nonce.clone();
            spawn_local(async move {
                if !alive.get() || navigation_generation.get() != generation {
                    return;
                }
                let callback_url = match source.callback_url() {
                    Ok(url) => url,
                    Err(error) => {
                        log::error!("could not resolve browser WebView navigation: {error:#}");
                        return;
                    }
                };
                if !navigation_allowed(&live, callback_url) {
                    return;
                }
                if !alive.get() || navigation_generation.get() != generation {
                    return;
                }
                if let Err(error) = load_frame_source(
                    &iframe,
                    &live,
                    &current_source,
                    &expected_origin,
                    &nonce,
                    source,
                ) {
                    log::error!("could not load browser WebView navigation: {error:#}");
                }
            });
            return Ok(());
        }
        load_frame_source(
            &self.iframe,
            &self.live,
            &self.current_source,
            &self.expected_origin,
            &self.nonce,
            source,
        )
    }

    fn apply_style(&self, parent_visible: bool) -> Result<()> {
        let live = self.live.borrow();
        let style = self.iframe.style();
        let visible = parent_visible && live.visible && !live.bounds.is_empty();
        style
            .set_property("display", if visible { "block" } else { "none" })
            .map_err(js_error)?;
        style
            .set_property("opacity", &live.opacity.clamp(0.0, 1.0).to_string())
            .map_err(js_error)?;
        if let Some(color) = live.background_color {
            style
                .set_property(
                    "background-color",
                    &format!(
                        "rgba({},{},{},{})",
                        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
                        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
                        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
                        color.a.clamp(0.0, 1.0)
                    ),
                )
                .map_err(js_error)?;
        } else {
            style
                .remove_property("background-color")
                .map_err(js_error)?;
        }
        Ok(())
    }

    fn position(&self, canvas_rect: &web_sys::DomRect, parent_visible: bool) {
        if self.apply_style(parent_visible).is_err() {
            return;
        }
        let live = self.live.borrow();
        let style = self.iframe.style();
        let _ = style.set_property(
            "left",
            &format!(
                "{}px",
                canvas_rect.left() + f64::from(live.bounds.origin.x.0)
            ),
        );
        let _ = style.set_property(
            "top",
            &format!(
                "{}px",
                canvas_rect.top() + f64::from(live.bounds.origin.y.0)
            ),
        );
        let _ = style.set_property("width", &format!("{}px", live.bounds.size.width.0.max(0.0)));
        let _ = style.set_property(
            "height",
            &format!("{}px", live.bounds.size.height.0.max(0.0)),
        );
    }

    fn message_source_matches(&self, event: &MessageEvent) -> bool {
        let Some(source) = event.source() else {
            return false;
        };
        let Some(frame_window) = self.iframe.content_window() else {
            return false;
        };
        if !js_sys::Object::is(source.as_ref(), frame_window.as_ref()) {
            return false;
        }
        match self.expected_origin.borrow().as_deref() {
            Some(expected) => event.origin() == expected,
            None => event.origin() == "null",
        }
    }

    fn apply_command(
        &self,
        canvas: &HtmlCanvasElement,
        command: PlatformWebViewCommand,
    ) -> Result<()> {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.load_source(SourceSignature::Url(url))
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                bail!(
                    "browser iframe WebViews cannot attach {} custom navigation header(s) to `{url}`",
                    headers.len()
                )
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.load_source(SourceSignature::Html(html))
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                frame_eval(&self.iframe, script.as_ref()).map(|_| ())
            }
            PlatformWebViewCommand::EvaluateJavaScriptWithResult {
                script, callback, ..
            } => {
                let result = frame_eval(&self.iframe, script.as_ref())
                    .and_then(|value| serialized_js_result(&value))
                    .map_err(|error| error.to_string().into());
                crate::platform::catch_platform_callback(
                    "browser",
                    "WebView JavaScript result",
                    (),
                    || callback(result),
                );
                Ok(())
            }
            PlatformWebViewCommand::PostMessage { message, .. } => {
                let frame = self
                    .iframe
                    .content_window()
                    .context("browser WebView frame Window is unavailable")?;
                let json = serde_json::to_string(&message)?;
                let payload = js_sys::JSON::parse(&json).map_err(js_error)?;
                let target = self.expected_origin.borrow();
                frame
                    .post_message(&payload, target.as_deref().unwrap_or("*"))
                    .map_err(js_error)
            }
            PlatformWebViewCommand::Reload { .. } => self
                .iframe
                .content_window()
                .context("browser WebView frame Window is unavailable")?
                .location()
                .reload()
                .map_err(js_error),
            PlatformWebViewCommand::GoBack { .. } => self
                .iframe
                .content_window()
                .context("browser WebView frame Window is unavailable")?
                .history()
                .map_err(js_error)?
                .back()
                .map_err(js_error),
            PlatformWebViewCommand::GoForward { .. } => self
                .iframe
                .content_window()
                .context("browser WebView frame Window is unavailable")?
                .history()
                .map_err(js_error)?
                .forward()
                .map_err(js_error),
            PlatformWebViewCommand::OpenDevTools { .. }
            | PlatformWebViewCommand::CloseDevTools { .. } => {
                bail!("iframe developer tools are controlled by the browser")
            }
            PlatformWebViewCommand::IsDevToolsOpen { callback, .. } => {
                callback(Err(
                    "iframe developer-tool state is not exposed by browsers".into(),
                ));
                Ok(())
            }
            PlatformWebViewCommand::Print { .. } => self
                .iframe
                .content_window()
                .context("browser WebView frame Window is unavailable")?
                .print()
                .map_err(js_error),
            PlatformWebViewCommand::SetZoomFactor { factor, .. } => {
                ensure!(
                    factor.is_finite() && (0.25..=5.0).contains(&factor),
                    "WebView zoom factor must be finite and between 0.25 and 5.0"
                );
                frame_eval(
                    &self.iframe,
                    &format!(
                        "document.documentElement.style.zoom = {};",
                        serde_json::to_string(&factor)?
                    ),
                )
                .map(|_| ())
            }
            PlatformWebViewCommand::Focus { .. } => self.iframe.focus().map_err(js_error),
            PlatformWebViewCommand::FocusParent { .. } => canvas.focus().map_err(js_error),
            PlatformWebViewCommand::ClearBrowsingData { .. } => bail!(
                "browsers do not expose profile-scoped browsing-data deletion to iframe hosts"
            ),
            PlatformWebViewCommand::ReadUrl { callback, .. } => {
                let result = frame_url(&self.iframe)
                    .or_else(|_| self.current_source.borrow().callback_url())
                    .map_err(|error| error.to_string().into());
                callback(result);
                Ok(())
            }
            PlatformWebViewCommand::ReadCookies { url, callback, .. } => {
                let result = read_frame_cookies(&self.iframe, url.as_ref())
                    .map_err(|error| error.to_string().into());
                callback(result);
                Ok(())
            }
            PlatformWebViewCommand::SetCookie {
                cookie, callback, ..
            } => {
                let result = set_frame_cookie(&self.iframe, &cookie, false)
                    .map_err(|error| error.to_string().into());
                callback(result);
                Ok(())
            }
            PlatformWebViewCommand::DeleteCookie {
                cookie, callback, ..
            } => {
                let result = set_frame_cookie(&self.iframe, &cookie, true)
                    .map_err(|error| error.to_string().into());
                callback(result);
                Ok(())
            }
        }
    }
}

impl Drop for BrowserWebViewHost {
    fn drop(&mut self) {
        self.alive.set(false);
        self.navigation_generation
            .set(self.navigation_generation.get().wrapping_add(1));
        let _ = self.iframe.remove_event_listener_with_callback(
            "load",
            self.load_callback.as_ref().unchecked_ref(),
        );
        self.iframe.remove();
    }
}

fn load_frame_source(
    iframe: &HtmlIFrameElement,
    live: &Rc<RefCell<PlatformWebView>>,
    current_source: &Rc<RefCell<SourceSignature>>,
    expected_origin: &Rc<RefCell<Option<String>>>,
    nonce: &str,
    source: SourceSignature,
) -> Result<()> {
    let callback_url = source.callback_url()?;
    dispatch_page_load(live, WebViewPageLoadEvent::Started, callback_url);

    let desired = live.borrow();
    match &source {
        SourceSignature::Url(url) => {
            let (url, origin) = resolve_url(url.as_ref())?;
            *expected_origin.borrow_mut() = expected_message_origin(&desired, origin);
            iframe.remove_attribute("srcdoc").map_err(js_error)?;
            iframe.set_src(&url);
        }
        SourceSignature::Html(html) => {
            *expected_origin.borrow_mut() = expected_inline_origin(&desired);
            iframe.remove_attribute("src").map_err(js_error)?;
            if desired.javascript_disabled {
                iframe.set_srcdoc(html.as_ref());
            } else {
                iframe.set_srcdoc(&inline_document(
                    html.as_ref(),
                    &bridge_script(&desired, nonce),
                ));
            }
        }
        SourceSignature::Empty => {
            *expected_origin.borrow_mut() = expected_inline_origin(&desired);
            iframe.remove_attribute("srcdoc").map_err(js_error)?;
            iframe.set_src("about:blank");
        }
    }
    *current_source.borrow_mut() = source;
    Ok(())
}

fn apply_frame_policy(iframe: &HtmlIFrameElement, desired: &PlatformWebView) -> Result<()> {
    let policy = &desired.browser_policy;
    iframe.set_referrer_policy(policy.referrer_policy.as_ref());
    iframe
        .set_attribute(
            "loading",
            match policy.loading {
                BrowserWebViewLoading::Eager => "eager",
                BrowserWebViewLoading::Lazy => "lazy",
            },
        )
        .map_err(js_error)?;
    if policy.credentialless {
        iframe
            .set_attribute("credentialless", "")
            .map_err(js_error)?;
    } else {
        iframe
            .remove_attribute("credentialless")
            .map_err(js_error)?;
    }

    match policy.sandbox {
        BrowserWebViewSandbox::Strict => {
            let mut tokens = vec!["allow-forms"];
            if policy.downloads {
                tokens.push("allow-downloads");
            }
            if !desired.javascript_disabled {
                tokens.push("allow-scripts");
            }
            iframe
                .set_attribute("sandbox", &tokens.join(" "))
                .map_err(js_error)?;
        }
        BrowserWebViewSandbox::TrustedSameOrigin => {
            let mut tokens = vec![
                "allow-forms",
                "allow-modals",
                "allow-popups",
                "allow-popups-to-escape-sandbox",
                "allow-same-origin",
                "allow-top-navigation-by-user-activation",
            ];
            if policy.downloads {
                tokens.push("allow-downloads");
            }
            if !desired.javascript_disabled {
                tokens.push("allow-scripts");
            }
            iframe
                .set_attribute("sandbox", &tokens.join(" "))
                .map_err(js_error)?;
        }
        BrowserWebViewSandbox::Unrestricted => {
            iframe.remove_attribute("sandbox").map_err(js_error)?;
        }
    }

    let source_origin = source_origin(&SourceSignature::from_webview(desired));
    let permission_origins = if policy.allowed_origins.is_empty() {
        source_origin.iter().map(String::as_str).collect::<Vec<_>>()
    } else {
        policy
            .allowed_origins
            .iter()
            .map(|origin| origin.as_ref())
            .collect::<Vec<_>>()
    };
    let mut permissions = HashSet::new();
    if policy.sandbox != BrowserWebViewSandbox::Strict {
        for kind in &policy.permissions {
            for name in permission_policy_names(*kind) {
                if !permission_origins.is_empty() {
                    permissions.insert(format!("{name} {}", permission_origins.join(" ")));
                }
            }
        }
        if desired.media_autoplay == Some(true) && !permission_origins.is_empty() {
            permissions.insert(format!("autoplay {}", permission_origins.join(" ")));
        }
        if desired.clipboard_access && !permission_origins.is_empty() {
            permissions.insert(format!("clipboard-read {}", permission_origins.join(" ")));
            permissions.insert(format!("clipboard-write {}", permission_origins.join(" ")));
        }
    }
    if permissions.is_empty() {
        iframe.remove_attribute("allow").map_err(js_error)?;
    } else {
        let mut permissions = permissions.into_iter().collect::<Vec<_>>();
        permissions.sort_unstable();
        iframe
            .set_attribute("allow", &permissions.join("; "))
            .map_err(js_error)?;
    }
    Ok(())
}

fn permission_policy_names(kind: WebViewPermissionKind) -> &'static [&'static str] {
    match kind {
        WebViewPermissionKind::Microphone => &["microphone"],
        WebViewPermissionKind::Camera => &["camera"],
        WebViewPermissionKind::Geolocation => &["geolocation"],
        WebViewPermissionKind::ClipboardRead => &["clipboard-read"],
        WebViewPermissionKind::DisplayCapture => &["display-capture"],
        WebViewPermissionKind::Midi => &["midi"],
        WebViewPermissionKind::Sensors => &["accelerometer", "gyroscope", "magnetometer"],
        WebViewPermissionKind::Autoplay => &["autoplay"],
        _ => &[],
    }
}

fn validate_source_policy(desired: &PlatformWebView, source: &SourceSignature) -> Result<()> {
    let policy = &desired.browser_policy;
    validate_browser_policy(policy)?;
    let SourceSignature::Url(url) = source else {
        return Ok(());
    };
    let (resolved, origin) = resolve_url(url.as_ref())?;
    let protocol = web_sys::Url::new(&resolved)
        .map_err(js_error)?
        .protocol()
        .to_ascii_lowercase();
    ensure!(
        matches!(
            protocol.as_str(),
            "http:" | "https:" | "about:" | "blob:" | "data:"
        ),
        "browser WebView URL scheme `{protocol}` is not allowed"
    );
    if !policy.allowed_origins.is_empty() {
        let candidate = origin.as_deref().unwrap_or("null");
        ensure!(
            policy
                .allowed_origins
                .iter()
                .any(|allowed| allowed.as_ref() == candidate),
            "browser WebView origin `{candidate}` is not in BrowserWebViewPolicy::allowed_origins"
        );
    }
    Ok(())
}

fn validate_browser_policy(policy: &crate::BrowserWebViewPolicy) -> Result<()> {
    ensure!(
        matches!(
            policy.referrer_policy.as_ref(),
            "" | "no-referrer"
                | "no-referrer-when-downgrade"
                | "origin"
                | "origin-when-cross-origin"
                | "same-origin"
                | "strict-origin"
                | "strict-origin-when-cross-origin"
                | "unsafe-url"
        ),
        "invalid browser WebView referrer policy `{}`",
        policy.referrer_policy
    );
    for allowed in &policy.allowed_origins {
        if allowed.as_ref() == "null" {
            continue;
        }
        let url = web_sys::Url::new(allowed.as_ref()).map_err(js_error)?;
        ensure!(
            matches!(url.protocol().as_str(), "http:" | "https:")
                && url.origin() == allowed.as_ref(),
            "browser WebView allowed origin `{allowed}` must be a canonical HTTP(S) origin without a path"
        );
    }
    Ok(())
}

fn source_origin(source: &SourceSignature) -> Option<String> {
    match source {
        SourceSignature::Url(url) => resolve_url(url.as_ref()).ok()?.1,
        SourceSignature::Html(_) | SourceSignature::Empty => parent_origin(),
    }
}

fn expected_message_origin(desired: &PlatformWebView, origin: Option<String>) -> Option<String> {
    (desired.browser_policy.sandbox != BrowserWebViewSandbox::Strict)
        .then_some(origin)
        .flatten()
}

fn expected_inline_origin(desired: &PlatformWebView) -> Option<String> {
    (desired.browser_policy.sandbox != BrowserWebViewSandbox::Strict)
        .then(parent_origin)
        .flatten()
}

fn resolve_url(url: &str) -> Result<(String, Option<String>)> {
    let window = web_sys::window().context("browser Window is unavailable")?;
    let base = window.location().href().map_err(js_error)?;
    let url = web_sys::Url::new_with_base(url, &base).map_err(js_error)?;
    let origin = url.origin();
    Ok((
        url.href(),
        (origin != "null" && !origin.is_empty()).then_some(origin),
    ))
}

fn parent_origin() -> Option<String> {
    web_sys::window()
        .and_then(|window| window.location().origin().ok())
        .filter(|origin| origin != "null" && !origin.is_empty())
}

fn bridge_script(desired: &PlatformWebView, nonce: &str) -> String {
    let instance = json_string(desired.instance_id.as_ref());
    let nonce = json_string(nonce);
    let target_origin = expected_inline_origin(desired)
        .map(|origin| json_string(&origin))
        .unwrap_or_else(|| "'*'".to_owned());
    let storage = desired
        .storage_key
        .as_ref()
        .map(|key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string(key.as_ref())
            )
        })
        .unwrap_or_default();
    format!(
        "(() => {{ {storage} const instance = {instance}; const nonce = {nonce}; const targetOrigin = {target_origin}; if (!window.external) window.external = {{}}; window.external.invoke = function(message) {{ const body = typeof message === 'string' ? message : JSON.stringify(message); parent.postMessage({{ {IPC_INSTANCE_FIELD}: instance, {IPC_NONCE_FIELD}: nonce, {IPC_BODY_FIELD}: body }}, targetOrigin); }}; if (!window.gpui) window.gpui = {{}}; window.gpui.postMessage = window.external.invoke; Object.defineProperty(window, '__KAEL_WEBVIEW_BRIDGE_READY__', {{ value: true, configurable: false }}); }})();"
    )
}

fn inline_document(html: &str, bootstrap: &str) -> String {
    let bootstrap = bootstrap.replace("</script", "<\\/script");
    format!("<!doctype html><script>{bootstrap}</script>{html}")
}

fn on_frame_loaded(
    iframe: &HtmlIFrameElement,
    live: &Rc<RefCell<PlatformWebView>>,
    current_source: &Rc<RefCell<SourceSignature>>,
    expected_origin: &Rc<RefCell<Option<String>>>,
    nonce: &str,
) {
    // A load callback can synchronously dispatch product callbacks and trigger
    // declarative WebView reconciliation. Work from a snapshot so no RefCell
    // guard crosses browser or application callbacks.
    let desired = live.borrow().clone();
    let source = current_source.borrow().clone();
    // A strict sandbox always creates a unique, opaque origin. Chromium returns
    // `None` for `contentDocument`, but WebKit also reports a page-level security
    // error merely for reading that property. Do not probe a boundary we already
    // know is inaccessible. The bridge for inline strict documents is embedded
    // in `srcdoc` before navigation and communicates only through `postMessage`.
    let accessible_document = if desired.browser_policy.sandbox == BrowserWebViewSandbox::Strict {
        None
    } else {
        iframe.content_document()
    };
    if !desired.javascript_disabled {
        // Inline documents receive this before their own parser scripts; this
        // installation also covers accessible same-origin URL documents.
        if accessible_document.is_some() {
            if let Err(error) = frame_eval(iframe, &bridge_script(&desired, nonce))
                && desired.browser_policy.cooperative_bridge_required
            {
                log::error!(
                    "browser WebView `{}` requires a cooperative bridge: {error:#}",
                    desired.id
                );
            }
        } else if desired.browser_policy.cooperative_bridge_required
            && !matches!(&source, SourceSignature::Html(_))
        {
            log::debug!(
                "browser WebView `{}` is waiting for its cross-origin cooperative bridge",
                desired.id
            );
        }
        if let Some(document) = accessible_document.as_ref() {
            for css in &desired.injected_css {
                if let Ok(style) = document.create_element("style") {
                    let _ = style.set_attribute("data-kael-webview-style", "true");
                    style.set_text_content(Some(css.as_ref()));
                    if let Ok(Some(head)) = document.query_selector("head") {
                        let _ = head.append_child(&style);
                    }
                }
            }
            for script in &desired.injected_javascript {
                if let Err(error) = frame_eval(iframe, script.as_ref()) {
                    log::warn!(
                        "could not inject JavaScript into browser WebView `{}`: {error:#}",
                        desired.id
                    );
                }
            }
        } else if !desired.injected_css.is_empty() || !desired.injected_javascript.is_empty() {
            log::warn!(
                "browser WebView `{}` cannot inject CSS or JavaScript into an opaque/cross-origin frame",
                desired.id
            );
        }
    }

    if accessible_document.is_some()
        && let Ok(url) = frame_url(iframe)
        && let Ok((_, origin)) = resolve_url(url.as_ref())
    {
        *expected_origin.borrow_mut() = origin;
    }
    let url = accessible_document
        .as_ref()
        .and_then(|_| frame_url(iframe).ok())
        .unwrap_or_else(|| {
            source
                .callback_url()
                .unwrap_or_else(|_| "about:blank".into())
        });
    if let Some(document) = accessible_document {
        dispatch_title_changed(live, document.title().into());
    }
    dispatch_page_load(live, WebViewPageLoadEvent::Finished, url);
}

fn dispatch_message(live: &Rc<RefCell<PlatformWebView>>, payload: serde_json::Value) {
    let (handler, mut async_window) = {
        let live = live.borrow();
        (live.message_handler.clone(), live.async_window.clone())
    };
    let Some(handler) = handler else { return };
    crate::platform::catch_platform_callback("browser", "WebView message", (), || {
        let _ = async_window.update(|window, cx| {
            handler(payload, window, cx);
            // Browser DOM callbacks arrive outside Kael's input/frame dispatch. A handler can
            // dirty a view without passing through the normal input path that re-arms rAF.
            window.update_frame_polling();
        });
    });
}

fn navigation_allowed(live: &Rc<RefCell<PlatformWebView>>, url: SharedString) -> bool {
    let (handler, mut async_window) = {
        let live = live.borrow();
        (live.navigation_handler.clone(), live.async_window.clone())
    };
    let Some(handler) = handler else { return true };
    crate::platform::catch_platform_callback(
        "browser",
        "WebView navigation policy",
        NavigationPolicy::Deny,
        || {
            async_window
                .update(|window, cx| handler(url, window, cx))
                .unwrap_or(NavigationPolicy::Deny)
        },
    ) == NavigationPolicy::Allow
}

fn dispatch_page_load(
    live: &Rc<RefCell<PlatformWebView>>,
    event: WebViewPageLoadEvent,
    url: SharedString,
) {
    let (handler, mut async_window) = {
        let live = live.borrow();
        (live.page_load_handler.clone(), live.async_window.clone())
    };
    let Some(handler) = handler else { return };
    crate::platform::catch_platform_callback("browser", "WebView page load", (), || {
        let _ = async_window.update(|window, cx| {
            handler(event, url, window, cx);
            window.update_frame_polling();
        });
    });
}

fn dispatch_title_changed(live: &Rc<RefCell<PlatformWebView>>, title: SharedString) {
    let (handler, mut async_window) = {
        let live = live.borrow();
        (
            live.document_title_changed_handler.clone(),
            live.async_window.clone(),
        )
    };
    let Some(handler) = handler else { return };
    crate::platform::catch_platform_callback("browser", "WebView title change", (), || {
        let _ = async_window.update(|window, cx| {
            handler(title, window, cx);
            window.update_frame_polling();
        });
    });
}

fn frame_eval(iframe: &HtmlIFrameElement, script: &str) -> Result<JsValue> {
    let frame = iframe
        .content_window()
        .context("browser WebView frame Window is unavailable")?;
    let eval = js_sys::Reflect::get(frame.as_ref(), &JsValue::from_str("eval"))
        .map_err(js_error)?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| anyhow!("browser WebView frame does not expose eval (cross-origin or CSP)"))?;
    eval.call1(frame.as_ref(), &JsValue::from_str(script))
        .map_err(js_error)
}

fn serialized_js_result(value: &JsValue) -> Result<SharedString> {
    if value.is_undefined() {
        return Ok("null".into());
    }
    js_sys::JSON::stringify(value)
        .map_err(js_error)?
        .as_string()
        .map(Into::into)
        .context("JavaScript result was not serializable")
}

fn frame_url(iframe: &HtmlIFrameElement) -> Result<SharedString> {
    iframe
        .content_window()
        .context("browser WebView frame Window is unavailable")?
        .location()
        .href()
        .map(Into::into)
        .map_err(js_error)
        .context("browser sandbox does not expose the iframe's current URL")
}

fn read_frame_cookies(
    iframe: &HtmlIFrameElement,
    requested_url: Option<&SharedString>,
) -> Result<Vec<WebViewCookie>> {
    if let Some(requested_url) = requested_url {
        let current = frame_url(iframe)?;
        ensure!(
            resolve_url(current.as_ref())?.1 == resolve_url(requested_url.as_ref())?.1,
            "browsers do not expose cookies for an origin other than the accessible iframe document"
        );
    }
    let value = frame_eval(iframe, "document.cookie")?
        .as_string()
        .context("iframe document.cookie was not a string")?;
    Ok(value
        .split(';')
        .filter_map(|entry| {
            let (name, value) = entry.trim().split_once('=')?;
            Some(WebViewCookie::new(name.to_owned(), value.to_owned()))
        })
        .collect())
}

fn set_frame_cookie(
    iframe: &HtmlIFrameElement,
    cookie: &WebViewCookie,
    delete: bool,
) -> Result<()> {
    ensure!(
        !cookie.http_only,
        "browser iframe hosts cannot create or delete HttpOnly cookies from JavaScript"
    );
    let value = if delete { "" } else { cookie.value.as_ref() };
    let mut serialized = format!("{}={value}", cookie.name);
    if let Some(domain) = &cookie.domain {
        serialized.push_str("; Domain=");
        serialized.push_str(domain);
    }
    if let Some(path) = &cookie.path {
        serialized.push_str("; Path=");
        serialized.push_str(path);
    }
    if cookie.secure {
        serialized.push_str("; Secure");
    }
    if delete {
        serialized.push_str("; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT");
    }
    frame_eval(
        iframe,
        &format!("document.cookie = {};", json_string(&serialized)),
    )?;
    Ok(())
}

fn message_json(event: &MessageEvent) -> Option<serde_json::Value> {
    let data = event.data();
    if let Some(data) = data.as_string() {
        serde_json::from_str(&data).ok()
    } else {
        let data = js_sys::JSON::stringify(&data).ok()?.as_string()?;
        serde_json::from_str(&data).ok()
    }
}

fn command_id(command: &PlatformWebViewCommand) -> SharedString {
    match command {
        PlatformWebViewCommand::Navigate { id, .. }
        | PlatformWebViewCommand::NavigateWithHeaders { id, .. }
        | PlatformWebViewCommand::LoadHtml { id, .. }
        | PlatformWebViewCommand::EvaluateJavaScript { id, .. }
        | PlatformWebViewCommand::EvaluateJavaScriptWithResult { id, .. }
        | PlatformWebViewCommand::PostMessage { id, .. }
        | PlatformWebViewCommand::Reload { id }
        | PlatformWebViewCommand::GoBack { id }
        | PlatformWebViewCommand::GoForward { id }
        | PlatformWebViewCommand::OpenDevTools { id }
        | PlatformWebViewCommand::CloseDevTools { id }
        | PlatformWebViewCommand::IsDevToolsOpen { id, .. }
        | PlatformWebViewCommand::Print { id }
        | PlatformWebViewCommand::SetZoomFactor { id, .. }
        | PlatformWebViewCommand::Focus { id }
        | PlatformWebViewCommand::FocusParent { id }
        | PlatformWebViewCommand::ClearBrowsingData { id }
        | PlatformWebViewCommand::ReadUrl { id, .. }
        | PlatformWebViewCommand::ReadCookies { id, .. }
        | PlatformWebViewCommand::SetCookie { id, .. }
        | PlatformWebViewCommand::DeleteCookie { id, .. } => id.clone(),
    }
}

fn set_document_webview_count(_local_count: usize) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let count = document
        .query_selector_all("iframe[data-kael-webview]")
        .map(|nodes| nodes.length())
        .unwrap_or_default();
    if let Some(root) = document.document_element() {
        let _ = root.set_attribute("data-kael-webview-count", &count.to_string());
    }
}

fn set_document_webview_message_received(canvas: &HtmlCanvasElement) {
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = root.set_attribute("data-kael-webview-message", "received");
        let message_count = root
            .get_attribute("data-kael-webview-message-count")
            .and_then(|count| count.parse::<u64>().ok())
            .unwrap_or_default()
            .saturating_add(1);
        let _ = root.set_attribute(
            "data-kael-webview-message-count",
            &message_count.to_string(),
        );
        let next_frame = canvas
            .get_attribute("data-kael-frame-count")
            .and_then(|count| count.parse::<u64>().ok())
            .unwrap_or_default()
            .saturating_add(1);
        let _ = root.set_attribute("data-kael-webview-message-frame", &next_frame.to_string());
    }
}

fn initialize_document_webview_message_count() {
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
        && !root.has_attribute("data-kael-webview-message-count")
    {
        let _ = root.set_attribute("data-kael-webview-message-count", "0");
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

#[cfg(test)]
mod tests {
    use super::SourceSignature;
    use crate::SharedString;

    #[test]
    fn imperative_navigation_does_not_change_declarative_baseline() {
        let rendered = SourceSignature::Url(SharedString::from("https://example.test/a"));
        let imperative = SourceSignature::Url(SharedString::from("https://example.test/b"));
        let unchanged_render = SourceSignature::Url(SharedString::from("https://example.test/a"));
        assert_ne!(imperative, rendered);
        assert_eq!(unchanged_render, rendered);

        let changed_render = SourceSignature::Url(SharedString::from("https://example.test/c"));
        assert_ne!(changed_render, rendered);
    }
}
