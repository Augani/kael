use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    NavigationPolicy, Pixels, Rgba, SharedString, Style, StyleRefinement, Styled,
    WebViewDownloadCompleted, WebViewDownloadPolicy, WebViewNewWindowPolicy, Window,
    webview::{
        PlatformWebView, WebViewCookie, WebViewDocumentTitleChangedHandler,
        WebViewDownloadCompletedHandler, WebViewDownloadStartedHandler, WebViewDragDropEvent,
        WebViewDragDropHandler, WebViewDragDropPolicy, WebViewMessageHandler,
        WebViewNavigationHandler, WebViewNewWindowHandler, WebViewPageLoadEvent,
        WebViewPageLoadHandler,
    },
};
use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use http_client::Url;
use refineable::Refineable;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

type WebViewPermissionRequestHandler =
    Rc<dyn Fn(WebViewPermissionRequest, &mut Window, &mut App) -> WebViewPermissionDecision>;

/// Creates a WebView element backed by the platform's native embedded web content view.
pub fn webview(id: impl Into<ElementId>, url: impl Into<SharedString>) -> WebView {
    WebView {
        element_id: id.into(),
        url: url.into(),
        style: StyleRefinement::default(),
        options: WebViewOptions::default(),
    }
}

/// Creates a WebView element with a reusable option bundle.
pub fn webview_with_options(
    id: impl Into<ElementId>,
    url: impl Into<SharedString>,
    options: WebViewOptions,
) -> WebView {
    webview(id, url).options(options)
}

/// Creates a WebView element from a local HTML/document file.
///
/// This is the WebView-island equivalent of browser-runtime `loadFile`. Relative
/// paths are resolved against the current working directory before being
/// converted to a `file://` URL.
pub fn webview_file(id: impl Into<ElementId>, path: impl AsRef<Path>) -> Result<WebView> {
    Ok(webview(id, webview_file_url(path)?))
}

/// Creates a local-file WebView with a reusable option bundle.
pub fn webview_file_with_options(
    id: impl Into<ElementId>,
    path: impl AsRef<Path>,
    options: WebViewOptions,
) -> Result<WebView> {
    Ok(webview_file(id, path)?.options(options))
}

/// Build a `file://` URL for a local WebView document.
pub fn webview_file_url(path: impl AsRef<Path>) -> Result<SharedString> {
    let path = path.as_ref();
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Url::from_file_path(&path)
        .map(|url| url.to_string().into())
        .map_err(|_| anyhow::anyhow!("could not convert path to file URL: {}", path.display()))
}

/// Creates a WebView element from an inline HTML document.
///
/// This is the WebView-island equivalent of browser-runtime `loadHTML`: useful for
/// controlled widgets, rich previews, demos, and small browser-only surfaces
/// that do not need a separate local server or asset file.
pub fn webview_html(id: impl Into<ElementId>, html: impl AsRef<str>) -> WebView {
    webview(id, "").html(html.as_ref().to_string())
}

/// Creates an inline HTML WebView with a reusable option bundle.
pub fn webview_html_with_options(
    id: impl Into<ElementId>,
    html: impl AsRef<str>,
    options: WebViewOptions,
) -> WebView {
    webview(id, "").options(options.html(html.as_ref().to_string()))
}

/// Build a `data:` URL for an inline HTML document.
pub fn webview_html_url(html: impl AsRef<str>) -> SharedString {
    format!(
        "data:text/html;charset=utf-8;base64,{}",
        BASE64.encode(html.as_ref())
    )
    .into()
}

/// Creates a controller for a WebView element with the given identifier.
pub fn webview_controller(id: impl Into<ElementId>) -> WebViewController {
    WebViewController::new(id)
}

/// A small command handle for an embedded WebView.
///
/// Keep this next to the element's id to avoid threading raw string ids through
/// app code when navigating, posting messages, or evaluating JavaScript.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WebViewController {
    id: SharedString,
}

/// Options for finding text inside a WebView document.
///
/// This is a lightweight, cross-backend wrapper over the browser
/// `window.find(...)` behavior. It is intended for app-owned find bars and
/// native desktop "find in page" commands that need basic next/previous
/// navigation without opening backend-native find UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WebViewFindOptions {
    /// Match letter case exactly.
    pub case_sensitive: bool,
    /// Search backwards from the current selection.
    pub backwards: bool,
    /// Wrap to the beginning/end of the document when needed.
    pub wrap: bool,
    /// Match whole words only where the browser supports it.
    pub whole_word: bool,
    /// Search frames where the browser supports it.
    pub search_in_frames: bool,
}

/// Result for an native desktop WebView find operation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewFindResult {
    /// Whether the browser selected a match for the query.
    #[serde(default)]
    pub found: bool,
    /// Number of DOM text matches counted in the current document.
    ///
    /// This is a portable app-side count for native find bars. It is not a
    /// backend-native match count and does not inspect cross-origin frames.
    #[serde(default)]
    pub matches: usize,
}

/// Event payload emitted by [`webview_find_result_bridge_script`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewFindEvent {
    /// Browser event name. Currently `"find"`.
    #[serde(default)]
    pub event: SharedString,
    /// Query passed to `window.find(...)`.
    #[serde(default)]
    pub query: SharedString,
    /// Whether the browser selected a match.
    #[serde(default)]
    pub found: bool,
    /// Portable DOM text match count for the current document.
    #[serde(default)]
    pub matches: usize,
    /// Match letter case exactly.
    #[serde(default, rename = "caseSensitive")]
    pub case_sensitive: bool,
    /// Whether the search moved backwards.
    #[serde(default)]
    pub backwards: bool,
    /// Whether the search wrapped.
    #[serde(default)]
    pub wrap: bool,
    /// Whole-word matching request.
    #[serde(default, rename = "wholeWord")]
    pub whole_word: bool,
    /// Search-frames request.
    #[serde(default, rename = "searchInFrames")]
    pub search_in_frames: bool,
    /// Browser-selected text after the find operation.
    #[serde(default, rename = "selectionText")]
    pub selection_text: SharedString,
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
}

impl WebViewFindEvent {
    /// Parse a bridge payload into a typed find event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed find event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// A heading discovered in a WebView document snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewDocumentHeading {
    /// Heading level, from `1` through `6`.
    #[serde(default)]
    pub level: u8,
    /// Heading text.
    #[serde(default)]
    pub text: SharedString,
    /// Element id, when present.
    #[serde(default)]
    pub id: Option<SharedString>,
}

/// A link discovered in a WebView document snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewDocumentLink {
    /// Link text.
    #[serde(default)]
    pub text: SharedString,
    /// Resolved link URL.
    #[serde(default)]
    pub href: SharedString,
    /// Link target attribute, when present.
    #[serde(default)]
    pub target: Option<SharedString>,
}

/// An image discovered in a WebView document snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewDocumentImage {
    /// Resolved image URL.
    #[serde(default)]
    pub src: SharedString,
    /// Image alt text.
    #[serde(default)]
    pub alt: SharedString,
    /// Image title attribute, when present.
    #[serde(default)]
    pub title: Option<SharedString>,
}

/// A form discovered in a WebView document snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewDocumentForm {
    /// Form id attribute, when present.
    #[serde(default)]
    pub id: Option<SharedString>,
    /// Form name attribute, when present.
    #[serde(default)]
    pub name: Option<SharedString>,
    /// Resolved form action URL.
    #[serde(default)]
    pub action: SharedString,
    /// Lowercase form method.
    #[serde(default)]
    pub method: SharedString,
    /// Number of controls in the form.
    #[serde(default, rename = "controlCount")]
    pub control_count: usize,
}

/// Structured snapshot of a WebView document for diagnostics and agents.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewDocumentSnapshot {
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Current document title.
    #[serde(default)]
    pub title: SharedString,
    /// Browser document ready state.
    #[serde(default, rename = "readyState")]
    pub ready_state: SharedString,
    /// Root language attribute, when present.
    #[serde(default)]
    pub language: Option<SharedString>,
    /// Root direction attribute, when present.
    #[serde(default)]
    pub direction: Option<SharedString>,
    /// Visible document text, truncated by the snapshot script.
    #[serde(default, rename = "visibleText")]
    pub visible_text: SharedString,
    /// Number of text characters before truncation.
    #[serde(default, rename = "textLength")]
    pub text_length: usize,
    /// Heading outline.
    #[serde(default)]
    pub headings: Vec<WebViewDocumentHeading>,
    /// Anchor links.
    #[serde(default)]
    pub links: Vec<WebViewDocumentLink>,
    /// Images.
    #[serde(default)]
    pub images: Vec<WebViewDocumentImage>,
    /// Forms.
    #[serde(default)]
    pub forms: Vec<WebViewDocumentForm>,
}

/// An attribute captured from a WebView DOM element snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewElementAttribute {
    /// Attribute name.
    #[serde(default)]
    pub name: SharedString,
    /// Attribute value.
    #[serde(default)]
    pub value: SharedString,
}

/// Bounding rectangle for a WebView DOM element in viewport CSS pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewElementRect {
    /// Left coordinate relative to the viewport.
    #[serde(default)]
    pub x: f64,
    /// Top coordinate relative to the viewport.
    #[serde(default)]
    pub y: f64,
    /// Element width in CSS pixels.
    #[serde(default)]
    pub width: f64,
    /// Element height in CSS pixels.
    #[serde(default)]
    pub height: f64,
}

/// Snapshot of one same-document WebView element selected by CSS selector.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewElementSnapshot {
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// CSS selector used for the query.
    #[serde(default)]
    pub selector: SharedString,
    /// Lowercase tag name such as `"button"`, `"img"`, or `"video"`.
    #[serde(default, rename = "tagName")]
    pub tag_name: SharedString,
    /// Element id, when present.
    #[serde(default)]
    pub id: Option<SharedString>,
    /// CSS classes in DOM token order.
    #[serde(default)]
    pub classes: Vec<SharedString>,
    /// Visible text content trimmed and whitespace-normalized.
    #[serde(default)]
    pub text: SharedString,
    /// Form-control value when the browser exposes one.
    #[serde(default)]
    pub value: Option<SharedString>,
    /// Checked state for checkbox/radio controls.
    #[serde(default)]
    pub checked: Option<bool>,
    /// Whether the element is disabled.
    #[serde(default)]
    pub disabled: bool,
    /// Whether the element is hidden by DOM/CSS visibility checks.
    #[serde(default)]
    pub hidden: bool,
    /// Whether the element is editable or inside an editable region.
    #[serde(default)]
    pub editable: bool,
    /// Resolved link URL for anchors or nearest link ancestors.
    #[serde(default)]
    pub href: Option<SharedString>,
    /// Resolved source URL for image/media elements.
    #[serde(default)]
    pub src: Option<SharedString>,
    /// Element rectangle in viewport CSS pixels.
    #[serde(default)]
    pub rect: WebViewElementRect,
    /// Element attributes.
    #[serde(default)]
    pub attributes: Vec<WebViewElementAttribute>,
    /// Computed CSS `display` value.
    #[serde(default)]
    pub display: SharedString,
    /// Computed CSS `visibility` value.
    #[serde(default)]
    pub visibility: SharedString,
    /// Computed CSS `pointer-events` value.
    #[serde(default, rename = "pointerEvents")]
    pub pointer_events: SharedString,
}

/// DOM-to-SVG capture options for [`WebViewController::capture_dom_image`].
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebViewDomImageCaptureOptions {
    /// Output width in CSS pixels. Defaults to the element's bounding width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Output height in CSS pixels. Defaults to the element's bounding height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// CSS background for the capture wrapper, such as `"#fff"` or `"transparent"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<SharedString>,
    /// Maximum output pixel area before Kael scales the SVG viewport down.
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxPixels")]
    pub max_pixels: Option<u32>,
}

impl WebViewDomImageCaptureOptions {
    /// Set the output size in CSS pixels.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Set the output width in CSS pixels.
    pub fn width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    /// Set the output height in CSS pixels.
    pub fn height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    /// Set the capture wrapper background.
    pub fn background(mut self, background: impl Into<SharedString>) -> Self {
        self.background = Some(background.into());
        self
    }

    /// Set the maximum output pixel area.
    pub fn max_pixels(mut self, max_pixels: u32) -> Self {
        self.max_pixels = Some(max_pixels);
        self
    }
}

/// Result for a WebView browser download trigger command.
///
/// This reports whether Kael successfully asked the hosted document to trigger
/// a browser download. The actual download is still governed by browser policy,
/// origin rules, response headers, and [`WebViewOptions::on_download_started`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewDownloadTriggerResult {
    /// Whether the browser download anchor was created and clicked.
    #[serde(default)]
    pub ok: bool,
    /// The URL resolved against the current document.
    #[serde(default)]
    pub url: SharedString,
    /// Requested download filename hint, when supplied.
    #[serde(default)]
    pub filename: Option<SharedString>,
    /// Browser error text when the trigger failed before dispatch.
    #[serde(default)]
    pub error: Option<SharedString>,
}

/// Event payload emitted by [`webview_favicon_bridge_script`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewFaviconEvent {
    /// Resolved favicon URLs discovered in document order.
    #[serde(default)]
    pub urls: Vec<SharedString>,
}

impl WebViewFaviconEvent {
    /// Parse a bridge payload into a typed favicon event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed favicon event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_location_bridge_script`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewLocationEvent {
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Current document title when available.
    #[serde(default)]
    pub title: SharedString,
    /// Browser document ready state such as `"loading"`, `"interactive"`, or `"complete"`.
    #[serde(default, rename = "readyState")]
    pub ready_state: SharedString,
    /// Whether same-document browser history suggests a Back action is possible.
    #[serde(default, rename = "canGoBack")]
    pub can_go_back: bool,
    /// Whether Kael's same-document navigation state bridge reports a Forward action.
    #[serde(default, rename = "canGoForward")]
    pub can_go_forward: bool,
}

impl WebViewLocationEvent {
    /// Parse a bridge payload into a typed location event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed location event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_lifecycle_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewLifecycleEvent {
    /// Browser event name such as `"focus"`, `"blur"`, `"visibilitychange"`,
    /// `"pageshow"`, `"pagehide"`, or `"fullscreenchange"`.
    #[serde(default)]
    pub event: SharedString,
    /// Current document visibility state.
    #[serde(default, rename = "visibilityState")]
    pub visibility_state: SharedString,
    /// Whether the document is hidden.
    #[serde(default)]
    pub hidden: bool,
    /// Whether the document currently has browser focus.
    #[serde(default, rename = "hasFocus")]
    pub has_focus: bool,
    /// Whether the hosted document currently owns browser fullscreen.
    #[serde(default)]
    pub fullscreen: bool,
    /// Whether a page show/hide event came from the back-forward cache.
    #[serde(default)]
    pub persisted: Option<bool>,
}

impl WebViewLifecycleEvent {
    /// Parse a bridge payload into a typed lifecycle event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed lifecycle event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_scroll_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewScrollEvent {
    /// Browser event name: `"initial"`, `"scroll"`, or `"resize"`.
    #[serde(default)]
    pub event: SharedString,
    /// Current horizontal scroll offset in CSS pixels.
    #[serde(default)]
    pub x: f64,
    /// Current vertical scroll offset in CSS pixels.
    #[serde(default)]
    pub y: f64,
    /// Maximum horizontal scroll offset in CSS pixels.
    #[serde(default, rename = "maxX")]
    pub max_x: f64,
    /// Maximum vertical scroll offset in CSS pixels.
    #[serde(default, rename = "maxY")]
    pub max_y: f64,
    /// Current viewport width in CSS pixels.
    #[serde(default, rename = "viewportWidth")]
    pub viewport_width: f64,
    /// Current viewport height in CSS pixels.
    #[serde(default, rename = "viewportHeight")]
    pub viewport_height: f64,
    /// Full scrollable document width in CSS pixels.
    #[serde(default, rename = "scrollWidth")]
    pub scroll_width: f64,
    /// Full scrollable document height in CSS pixels.
    #[serde(default, rename = "scrollHeight")]
    pub scroll_height: f64,
    /// Horizontal scroll progress from `0.0` to `1.0`.
    #[serde(default, rename = "progressX")]
    pub progress_x: f64,
    /// Vertical scroll progress from `0.0` to `1.0`.
    #[serde(default, rename = "progressY")]
    pub progress_y: f64,
}

impl WebViewScrollEvent {
    /// Parse a bridge payload into a typed scroll event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed scroll event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_selection_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewSelectionEvent {
    /// Browser event name such as `"initial"`, `"selectionchange"`, or `"select"`.
    #[serde(default)]
    pub event: SharedString,
    /// Plain selected text.
    #[serde(default, rename = "selectedText")]
    pub selected_text: SharedString,
    /// Selected HTML for document selections, or escaped selected text for inputs.
    #[serde(default, rename = "selectedHtml")]
    pub selected_html: SharedString,
    /// Whether the current selection is collapsed or empty.
    #[serde(default)]
    pub collapsed: bool,
    /// Whether the active selection target is editable.
    #[serde(default)]
    pub editable: bool,
    /// Active input kind such as `"input"`, `"textarea"`, or `"contenteditable"`.
    #[serde(default, rename = "inputKind")]
    pub input_kind: Option<SharedString>,
}

impl WebViewSelectionEvent {
    /// Parse a bridge payload into a typed selection event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed selection event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_console_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewConsoleEvent {
    /// Console level such as `"log"`, `"info"`, `"warn"`, `"error"`, or `"debug"`.
    #[serde(default)]
    pub level: SharedString,
    /// Human-readable message assembled from the console arguments.
    #[serde(default)]
    pub message: SharedString,
    /// JSON-safe snapshots of the original console arguments.
    #[serde(default)]
    pub args: Vec<serde_json::Value>,
    /// Page URL where the console event was observed.
    #[serde(default)]
    pub source: Option<SharedString>,
    /// Source line for `window.onerror` events, when available.
    #[serde(default)]
    pub line: Option<u32>,
    /// Source column for `window.onerror` events, when available.
    #[serde(default)]
    pub column: Option<u32>,
}

impl WebViewConsoleEvent {
    /// Parse a bridge payload into a typed console event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed console event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_keyboard_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewKeyboardEvent {
    /// Browser event name: `"keydown"`, `"keyup"`, or `"beforeinput"`.
    #[serde(default)]
    pub event: SharedString,
    /// Keyboard key value for key events.
    #[serde(default)]
    pub key: Option<SharedString>,
    /// Physical keyboard code for key events.
    #[serde(default)]
    pub code: Option<SharedString>,
    /// Keyboard location for key events.
    #[serde(default)]
    pub location: u32,
    /// Whether the key event is repeating.
    #[serde(default)]
    pub repeat: bool,
    /// Whether the event is part of IME composition.
    #[serde(default, rename = "isComposing")]
    pub is_composing: bool,
    /// Whether Alt/Option is pressed.
    #[serde(default, rename = "altKey")]
    pub alt_key: bool,
    /// Whether Ctrl is pressed.
    #[serde(default, rename = "ctrlKey")]
    pub ctrl_key: bool,
    /// Whether Meta/Command/Windows is pressed.
    #[serde(default, rename = "metaKey")]
    pub meta_key: bool,
    /// Whether Shift is pressed.
    #[serde(default, rename = "shiftKey")]
    pub shift_key: bool,
    /// Whether the event target is editable.
    #[serde(default, rename = "targetEditable")]
    pub target_editable: bool,
    /// Browser beforeinput input type.
    #[serde(default, rename = "inputType")]
    pub input_type: Option<SharedString>,
    /// Browser beforeinput text payload.
    #[serde(default)]
    pub data: Option<SharedString>,
    /// Whether page code already prevented the event default.
    #[serde(default, rename = "defaultPrevented")]
    pub default_prevented: bool,
}

impl WebViewKeyboardEvent {
    /// Parse a bridge payload into a typed keyboard event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed keyboard event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Action to perform when stopping an active WebView find session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebViewStopFindAction {
    /// Clear the current browser selection and blur the active element.
    #[default]
    ClearSelection,
    /// Leave the current browser selection in place.
    KeepSelection,
    /// Focus and scroll the currently selected match into view when possible.
    ActivateSelection,
}

impl WebViewFindOptions {
    /// Search forward, wrapping around the document.
    pub fn forward() -> Self {
        Self {
            wrap: true,
            search_in_frames: true,
            ..Self::default()
        }
    }

    /// Search backward, wrapping around the document.
    pub fn backward() -> Self {
        Self {
            backwards: true,
            wrap: true,
            search_in_frames: true,
            ..Self::default()
        }
    }

    /// Enable or disable case-sensitive matching.
    pub fn case_sensitive(mut self, enabled: bool) -> Self {
        self.case_sensitive = enabled;
        self
    }

    /// Enable or disable backwards search.
    pub fn backwards(mut self, enabled: bool) -> Self {
        self.backwards = enabled;
        self
    }

    /// Enable or disable wrapping.
    pub fn wrap(mut self, enabled: bool) -> Self {
        self.wrap = enabled;
        self
    }

    /// Enable or disable whole-word matching where the browser supports it.
    pub fn whole_word(mut self, enabled: bool) -> Self {
        self.whole_word = enabled;
        self
    }

    /// Enable or disable searching frames where the browser supports it.
    pub fn search_in_frames(mut self, enabled: bool) -> Self {
        self.search_in_frames = enabled;
        self
    }
}

/// Browser edit command for hosted inputs and editable documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebViewEditCommand {
    /// Copy the current browser selection.
    Copy,
    /// Cut the current browser selection.
    Cut,
    /// Paste from the system clipboard when the backend/browser allows it.
    Paste,
    /// Select all editable/browser document content.
    SelectAll,
    /// Undo the last browser editing action.
    Undo,
    /// Redo the last undone browser editing action.
    Redo,
    /// Delete the current selection.
    Delete,
}

impl WebViewEditCommand {
    fn document_command(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::SelectAll => "selectAll",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Delete => "delete",
        }
    }
}

/// A buffered media time range reported by a browser `<audio>` or `<video>` element.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewMediaTimeRange {
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
}

/// A browser text-track cue active for a WebView media element.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewMediaTextCue {
    /// Cue id when the browser exposes one.
    #[serde(default)]
    pub id: Option<SharedString>,
    /// Start time in seconds.
    #[serde(default, rename = "startTime")]
    pub start_time: f64,
    /// End time in seconds.
    #[serde(default, rename = "endTime")]
    pub end_time: f64,
    /// Cue text.
    #[serde(default)]
    pub text: SharedString,
}

/// Browser text-track state for a WebView media element.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewMediaTextTrackState {
    /// Zero-based index in `element.textTracks`.
    #[serde(default)]
    pub index: usize,
    /// Track id when present.
    #[serde(default)]
    pub id: Option<SharedString>,
    /// Track kind, such as `"subtitles"` or `"captions"`.
    #[serde(default)]
    pub kind: SharedString,
    /// Human-readable track label.
    #[serde(default)]
    pub label: SharedString,
    /// Track language.
    #[serde(default)]
    pub language: SharedString,
    /// Browser track mode: `"disabled"`, `"hidden"`, or `"showing"`.
    #[serde(default)]
    pub mode: SharedString,
    /// Active cues currently reported by the browser for this track.
    #[serde(default, rename = "activeCues")]
    pub active_cues: Vec<WebViewMediaTextCue>,
}

/// Snapshot of a browser `<audio>` or `<video>` element inside a WebView.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewMediaElementState {
    /// Zero-based index in `document.querySelectorAll("audio,video")`.
    #[serde(default)]
    pub index: usize,
    /// Lowercase element tag name, usually `"audio"` or `"video"`.
    #[serde(default, rename = "tagName")]
    pub tag_name: SharedString,
    /// DOM id when present.
    #[serde(default)]
    pub id: Option<SharedString>,
    /// Current resolved media source when available.
    #[serde(default)]
    pub src: Option<SharedString>,
    /// Whether playback is currently paused.
    #[serde(default)]
    pub paused: bool,
    /// Whether playback has reached the end.
    #[serde(default)]
    pub ended: bool,
    /// Whether the element is muted.
    #[serde(default)]
    pub muted: bool,
    /// Browser media volume in the `0.0..=1.0` range.
    #[serde(default)]
    pub volume: f64,
    /// Current playback rate.
    #[serde(default, rename = "playbackRate")]
    pub playback_rate: f64,
    /// Current playback position in seconds.
    #[serde(default, rename = "currentTime")]
    pub current_time: f64,
    /// Duration in seconds, or `None` when the browser reports `NaN`/infinity.
    #[serde(default)]
    pub duration: Option<f64>,
    /// media playback surface readyState.
    #[serde(default, rename = "readyState")]
    pub ready_state: u16,
    /// media playback surface networkState.
    #[serde(default, rename = "networkState")]
    pub network_state: u16,
    /// Whether the element is currently seeking.
    #[serde(default)]
    pub seeking: bool,
    /// Whether this element is the document fullscreen element.
    #[serde(default)]
    pub fullscreen: bool,
    /// Whether this element is the active picture-in-picture element.
    #[serde(default, rename = "pictureInPicture")]
    pub picture_in_picture: bool,
    /// Buffered ranges reported by the browser.
    #[serde(default)]
    pub buffered: Vec<WebViewMediaTimeRange>,
    /// Browser text tracks and their active cues.
    #[serde(default, rename = "textTracks")]
    pub text_tracks: Vec<WebViewMediaTextTrackState>,
}

/// Browser media element options applied by [`WebViewController::set_media_options`].
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebViewMediaElementOptions {
    /// Show or hide browser-provided media controls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controls: Option<bool>,
    /// Toggle browser loop behavior.
    #[serde(skip_serializing_if = "Option::is_none", rename = "loop")]
    pub loop_enabled: Option<bool>,
    /// Toggle the browser autoplay property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoplay: Option<bool>,
    /// Toggle the muted property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    /// Toggle inline video playback on platforms that distinguish it.
    #[serde(skip_serializing_if = "Option::is_none", rename = "playsInline")]
    pub plays_inline: Option<bool>,
    /// Set the poster image URL for video elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<SharedString>,
    /// Set the browser preload hint, such as `"none"`, `"metadata"`, or `"auto"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preload: Option<SharedString>,
    /// Replace browser controls-list tokens such as `"nodownload"`.
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "controlsList")]
    pub controls_list: Vec<SharedString>,
    /// Toggle browser picture-in-picture disablement when supported.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "disablePictureInPicture"
    )]
    pub disable_picture_in_picture: Option<bool>,
}

impl WebViewMediaElementOptions {
    /// Set whether browser-provided media controls are visible.
    pub fn controls(mut self, controls: bool) -> Self {
        self.controls = Some(controls);
        self
    }

    /// Set whether browser loop behavior is enabled.
    pub fn loop_enabled(mut self, loop_enabled: bool) -> Self {
        self.loop_enabled = Some(loop_enabled);
        self
    }

    /// Set whether the browser autoplay property is enabled.
    pub fn autoplay(mut self, autoplay: bool) -> Self {
        self.autoplay = Some(autoplay);
        self
    }

    /// Set whether the media element is muted.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = Some(muted);
        self
    }

    /// Set whether inline video playback is requested.
    pub fn plays_inline(mut self, plays_inline: bool) -> Self {
        self.plays_inline = Some(plays_inline);
        self
    }

    /// Set the poster image URL for video elements.
    pub fn poster(mut self, poster: impl Into<SharedString>) -> Self {
        self.poster = Some(poster.into());
        self
    }

    /// Set the browser preload hint.
    pub fn preload(mut self, preload: impl Into<SharedString>) -> Self {
        self.preload = Some(preload.into());
        self
    }

    /// Replace browser controls-list tokens.
    pub fn controls_list<I, S>(mut self, controls_list: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SharedString>,
    {
        self.controls_list = controls_list.into_iter().map(Into::into).collect();
        self
    }

    /// Toggle browser picture-in-picture disablement.
    pub fn disable_picture_in_picture(mut self, disabled: bool) -> Self {
        self.disable_picture_in_picture = Some(disabled);
        self
    }
}

/// Canvas capture options for [`WebViewController::capture_media_frame`].
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebViewMediaFrameCaptureOptions {
    /// Output width in pixels. Defaults to the video's intrinsic frame width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Output height in pixels. Defaults to the video's intrinsic frame height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Canvas MIME type such as `"image/png"` or `"image/jpeg"`.
    #[serde(skip_serializing_if = "Option::is_none", rename = "mimeType")]
    pub mime_type: Option<SharedString>,
    /// Encoder quality for lossy formats, clamped by the browser to `0.0..=1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f32>,
}

impl WebViewMediaFrameCaptureOptions {
    /// Set the output size in pixels.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Set the output width in pixels.
    pub fn width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    /// Set the output height in pixels.
    pub fn height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    /// Set the canvas MIME type.
    pub fn mime_type(mut self, mime_type: impl Into<SharedString>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Set encoder quality for lossy formats.
    pub fn quality(mut self, quality: f32) -> Self {
        self.quality = Some(quality);
        self
    }
}

/// Selector-scoped browser media command for [`WebViewController::media_command`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WebViewMediaCommand {
    /// Call `play()` on the matching media element.
    Play,
    /// Call `pause()` on the matching media element.
    Pause,
    /// Toggle between `play()` and `pause()`.
    TogglePlay,
    /// Pause playback and seek to the start.
    Stop,
    /// Set the `muted` property.
    SetMuted(bool),
    /// Set `volume`, clamped to `0.0..=1.0`.
    SetVolume(f32),
    /// Set `playbackRate`; values below zero are clamped to `0.0`.
    SetPlaybackRate(f32),
    /// Set `currentTime` in seconds; negative or non-finite values become `0.0`.
    SeekSecs(f64),
}

impl WebViewMediaCommand {
    fn json(self) -> String {
        match self {
            Self::Play => r#"{"action":"play"}"#.into(),
            Self::Pause => r#"{"action":"pause"}"#.into(),
            Self::TogglePlay => r#"{"action":"togglePlay"}"#.into(),
            Self::Stop => r#"{"action":"stop"}"#.into(),
            Self::SetMuted(muted) => format!(
                r#"{{"action":"setMuted","value":{}}}"#,
                if muted { "true" } else { "false" }
            ),
            Self::SetVolume(volume) => {
                let volume = if volume.is_finite() { volume } else { 1.0 };
                format!(
                    r#"{{"action":"setVolume","value":{}}}"#,
                    volume.clamp(0.0, 1.0)
                )
            }
            Self::SetPlaybackRate(rate) => {
                let rate = if rate.is_finite() { rate.max(0.0) } else { 1.0 };
                format!(r#"{{"action":"setPlaybackRate","value":{rate}}}"#)
            }
            Self::SeekSecs(seconds) => {
                let seconds = if seconds.is_finite() {
                    seconds.max(0.0)
                } else {
                    0.0
                };
                format!(r#"{{"action":"seek","value":{seconds}}}"#)
            }
        }
    }
}

/// Browser `<track>` options added by [`WebViewController::add_media_text_track`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebViewMediaTextTrackOptions {
    /// Browser-readable track source URL, usually a WebVTT URL or data URL.
    pub src: SharedString,
    /// HTML track kind such as `"subtitles"`, `"captions"`, or `"chapters"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<SharedString>,
    /// User-facing track label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<SharedString>,
    /// BCP-47-ish language tag when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<SharedString>,
    /// DOM id to assign to the created `<track>` element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<SharedString>,
    /// Mark this track as the browser default.
    #[serde(skip_serializing_if = "Option::is_none", rename = "default")]
    pub default_track: Option<bool>,
    /// Initial browser TextTrack mode: `"disabled"`, `"hidden"`, or `"showing"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<SharedString>,
}

impl WebViewMediaTextTrackOptions {
    /// Create a WebVTT subtitles track from a URL or data URL.
    pub fn webvtt(src: impl Into<SharedString>) -> Self {
        Self {
            src: src.into(),
            kind: Some("subtitles".into()),
            label: None,
            language: None,
            id: None,
            default_track: None,
            mode: None,
        }
    }

    /// Set the HTML track kind.
    pub fn kind(mut self, kind: impl Into<SharedString>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Set the user-facing label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the language tag.
    pub fn language(mut self, language: impl Into<SharedString>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Set the DOM id.
    pub fn id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Mark this track as the browser default.
    pub fn default_track(mut self, default_track: bool) -> Self {
        self.default_track = Some(default_track);
        self
    }

    /// Set the initial TextTrack mode.
    pub fn mode(mut self, mode: impl Into<SharedString>) -> Self {
        self.mode = Some(mode.into());
        self
    }
}

/// Event payload emitted by [`webview_media_event_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewMediaEvent {
    /// Browser media event name, such as `"play"`, `"pause"`, `"timeupdate"`, or `"ended"`.
    #[serde(default)]
    pub event: SharedString,
    /// Snapshot of the media element that emitted the event.
    #[serde(default)]
    pub state: WebViewMediaElementState,
}

impl WebViewMediaEvent {
    /// Parse a bridge payload into a typed media event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed media event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_context_menu_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewContextMenuEvent {
    /// Pointer X coordinate in the hosted page viewport.
    #[serde(default)]
    pub x: f64,
    /// Pointer Y coordinate in the hosted page viewport.
    #[serde(default)]
    pub y: f64,
    /// Currently selected text in the hosted page, including focused inputs.
    #[serde(default, rename = "selectedText")]
    pub selected_text: SharedString,
    /// Resolved href for the nearest clicked link, when any.
    #[serde(default, rename = "linkHref")]
    pub link_href: Option<SharedString>,
    /// Resolved source for the nearest clicked image, when any.
    #[serde(default, rename = "imageSrc")]
    pub image_src: Option<SharedString>,
    /// Resolved source for the nearest clicked audio/video element, when any.
    #[serde(default, rename = "mediaSrc")]
    pub media_src: Option<SharedString>,
    /// Whether the clicked target is inside an editable control or region.
    #[serde(default)]
    pub editable: bool,
    /// Input type or tag name for the nearest clicked editable control.
    #[serde(default, rename = "inputKind")]
    pub input_kind: Option<SharedString>,
}

impl WebViewContextMenuEvent {
    /// Parse a bridge payload into a typed context-menu event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed context-menu event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_pointer_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewPointerEvent {
    /// Browser pointer/mouse event name such as `"pointermove"`, `"click"`, or `"pointerleave"`.
    #[serde(default)]
    pub event: SharedString,
    /// Pointer X coordinate in the hosted page viewport.
    #[serde(default)]
    pub x: f64,
    /// Pointer Y coordinate in the hosted page viewport.
    #[serde(default)]
    pub y: f64,
    /// Pressed pointer buttons bitfield from the browser event.
    #[serde(default)]
    pub buttons: u16,
    /// Browser pointer type such as `"mouse"`, `"pen"`, or `"touch"`.
    #[serde(default, rename = "pointerType")]
    pub pointer_type: SharedString,
    /// Uppercase tag name for the direct event target, when available.
    #[serde(default, rename = "targetTag")]
    pub target_tag: Option<SharedString>,
    /// Resolved href for the nearest hovered/clicked link, when any.
    #[serde(default, rename = "linkHref")]
    pub link_href: Option<SharedString>,
    /// Resolved source for the nearest hovered/clicked image, when any.
    #[serde(default, rename = "imageSrc")]
    pub image_src: Option<SharedString>,
    /// Resolved source for the nearest hovered/clicked media element, when any.
    #[serde(default, rename = "mediaSrc")]
    pub media_src: Option<SharedString>,
    /// Whether the target is inside an editable control or region.
    #[serde(default)]
    pub editable: bool,
    /// Input kind such as `"text"`, `"textarea"`, or `"contenteditable"`.
    #[serde(default, rename = "inputKind")]
    pub input_kind: Option<SharedString>,
}

impl WebViewPointerEvent {
    /// Parse a bridge payload into a typed pointer event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed pointer event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Snapshot of a browser form control emitted by [`webview_form_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewFormControlState {
    /// Control `name` attribute, when present.
    #[serde(default)]
    pub name: Option<SharedString>,
    /// Control `id` attribute, when present.
    #[serde(default)]
    pub id: Option<SharedString>,
    /// Lowercase tag name such as `"input"`, `"textarea"`, or `"select"`.
    #[serde(default, rename = "tagName")]
    pub tag_name: SharedString,
    /// Input type or control kind such as `"text"`, `"checkbox"`, or `"select"`.
    #[serde(default, rename = "inputKind")]
    pub input_kind: SharedString,
    /// Current value for non-sensitive controls.
    ///
    /// Password and file inputs intentionally report `None`.
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    /// Checked state for checkbox and radio controls.
    #[serde(default)]
    pub checked: Option<bool>,
    /// Whether the control is disabled.
    #[serde(default)]
    pub disabled: bool,
    /// Whether the control is required.
    #[serde(default)]
    pub required: bool,
}

/// Event payload emitted by [`webview_form_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewFormEvent {
    /// Browser form event name: `"submit"`, `"reset"`, `"change"`, or `"input"`.
    #[serde(default)]
    pub event: SharedString,
    /// Form `id` attribute, when present.
    #[serde(default, rename = "formId")]
    pub form_id: Option<SharedString>,
    /// Form `name` attribute, when present.
    #[serde(default, rename = "formName")]
    pub form_name: Option<SharedString>,
    /// Resolved form action URL.
    #[serde(default)]
    pub action: Option<SharedString>,
    /// Lowercase form method, usually `"get"` or `"post"`.
    #[serde(default)]
    pub method: SharedString,
    /// Form target attribute.
    #[serde(default)]
    pub target: Option<SharedString>,
    /// Form encoding type.
    #[serde(default)]
    pub enctype: Option<SharedString>,
    /// Control that triggered a change/input event or submitter button when known.
    #[serde(default)]
    pub field: Option<WebViewFormControlState>,
    /// Form controls at submit/reset time.
    #[serde(default)]
    pub fields: Vec<WebViewFormControlState>,
    /// Whether page code had prevented the event default before Kael observed it.
    #[serde(default, rename = "defaultPrevented")]
    pub default_prevented: bool,
}

impl WebViewFormEvent {
    /// Parse a bridge payload into a typed form event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed form event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Browser file metadata from an `<input type="file">` selection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewFileInputFile {
    /// Browser-exposed file name. Browsers do not expose the local path.
    #[serde(default)]
    pub name: SharedString,
    /// File size in bytes.
    #[serde(default)]
    pub size: u64,
    /// Browser-reported MIME type, when available.
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<SharedString>,
    /// Browser-reported last modified timestamp in milliseconds since Unix epoch.
    #[serde(default, rename = "lastModified")]
    pub last_modified: Option<u64>,
}

/// Event payload emitted by [`webview_file_input_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewFileInputEvent {
    /// Browser event name: `"change"` or `"input"`.
    #[serde(default)]
    pub event: SharedString,
    /// File input `name` attribute, when present.
    #[serde(default, rename = "inputName")]
    pub input_name: Option<SharedString>,
    /// File input `id` attribute, when present.
    #[serde(default, rename = "inputId")]
    pub input_id: Option<SharedString>,
    /// File input `accept` attribute, when present.
    #[serde(default)]
    pub accept: Option<SharedString>,
    /// Whether the input allows multiple files.
    #[serde(default)]
    pub multiple: bool,
    /// Owning form `id` attribute, when present.
    #[serde(default, rename = "formId")]
    pub form_id: Option<SharedString>,
    /// Owning form `name` attribute, when present.
    #[serde(default, rename = "formName")]
    pub form_name: Option<SharedString>,
    /// Resolved form action URL.
    #[serde(default)]
    pub action: Option<SharedString>,
    /// Lowercase form method, usually `"get"` or `"post"`.
    #[serde(default)]
    pub method: SharedString,
    /// Selected file metadata.
    #[serde(default)]
    pub files: Vec<WebViewFileInputFile>,
}

impl WebViewFileInputEvent {
    /// Parse a bridge payload into a typed file-input event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed file-input event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_resource_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewResourceEvent {
    /// Event source: `"resource"` for PerformanceResourceTiming entries or
    /// browser event names such as `"load"` / `"error"` for element events.
    #[serde(default)]
    pub event: SharedString,
    /// Resource URL.
    #[serde(default)]
    pub url: SharedString,
    /// Browser initiator type such as `"script"`, `"img"`, `"css"`, or `"fetch"`.
    #[serde(default, rename = "initiatorType")]
    pub initiator_type: SharedString,
    /// Uppercase DOM tag name for element load/error events, when available.
    #[serde(default, rename = "targetTag")]
    pub target_tag: Option<SharedString>,
    /// Whether an element load/error event succeeded. Performance entries use `None`.
    #[serde(default)]
    pub success: Option<bool>,
    /// Performance entry start time in milliseconds from time origin.
    #[serde(default, rename = "startTime")]
    pub start_time: f64,
    /// Performance entry duration in milliseconds.
    #[serde(default)]
    pub duration: f64,
    /// Transferred byte size when the browser exposes it.
    #[serde(default, rename = "transferSize")]
    pub transfer_size: u64,
    /// Encoded body size when the browser exposes it.
    #[serde(default, rename = "encodedBodySize")]
    pub encoded_body_size: u64,
    /// Decoded body size when the browser exposes it.
    #[serde(default, rename = "decodedBodySize")]
    pub decoded_body_size: u64,
    /// Network protocol reported by the browser, such as `"h2"` or `"http/1.1"`.
    #[serde(default, rename = "nextHopProtocol")]
    pub next_hop_protocol: Option<SharedString>,
    /// Browser render-blocking status when available.
    #[serde(default, rename = "renderBlockingStatus")]
    pub render_blocking_status: Option<SharedString>,
}

impl WebViewResourceEvent {
    /// Parse a bridge payload into a typed resource event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed resource event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_network_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewNetworkEvent {
    /// Event source such as `"fetch"`, `"fetch-error"`, `"xhr"`, `"xhr-error"`, `"xhr-abort"`, or `"xhr-timeout"`.
    #[serde(default)]
    pub event: SharedString,
    /// Browser API that produced the event: `"fetch"` or `"XMLHttpRequest"`.
    #[serde(default)]
    pub api: SharedString,
    /// Uppercase HTTP method when known.
    #[serde(default)]
    pub method: SharedString,
    /// Request URL.
    #[serde(default)]
    pub url: SharedString,
    /// HTTP status code when a response was produced.
    #[serde(default)]
    pub status: Option<u16>,
    /// HTTP status text when a response was produced.
    #[serde(default, rename = "statusText")]
    pub status_text: Option<SharedString>,
    /// Whether the response status was in the browser's OK range.
    #[serde(default)]
    pub ok: Option<bool>,
    /// Elapsed time in milliseconds measured around the API call.
    #[serde(default, rename = "durationMs")]
    pub duration_ms: f64,
    /// Error name for rejected/failed requests, when available.
    #[serde(default, rename = "errorName")]
    pub error_name: Option<SharedString>,
    /// Error message for rejected/failed requests, when available.
    #[serde(default, rename = "errorMessage")]
    pub error_message: Option<SharedString>,
    /// XHR response type when available.
    #[serde(default, rename = "responseType")]
    pub response_type: Option<SharedString>,
    /// Current document URL.
    #[serde(default, rename = "documentUrl")]
    pub document_url: SharedString,
}

impl WebViewNetworkEvent {
    /// Parse a bridge payload into a typed network event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed network event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_dialog_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewDialogEvent {
    /// Browser dialog event: `"alert"`, `"confirm"`, `"prompt"`, or `"beforeunload"`.
    #[serde(default)]
    pub event: SharedString,
    /// Dialog message or beforeunload return value.
    #[serde(default)]
    pub message: SharedString,
    /// Prompt default value, when available.
    #[serde(default, rename = "defaultValue")]
    pub default_value: Option<SharedString>,
    /// Dialog return value after browser handling. `confirm` reports a boolean,
    /// `prompt` reports a string or null, and `alert` / `beforeunload` report null.
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Whether page code prevented default before Kael observed the event.
    #[serde(default, rename = "defaultPrevented")]
    pub default_prevented: bool,
}

impl WebViewDialogEvent {
    /// Parse a bridge payload into a typed dialog event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed dialog event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_clipboard_event_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewClipboardEvent {
    /// Browser clipboard event: `"copy"`, `"cut"`, or `"paste"`.
    #[serde(default)]
    pub event: SharedString,
    /// Clipboard MIME/data types advertised by the browser event.
    #[serde(default)]
    pub types: Vec<SharedString>,
    /// Plain text clipboard data when the browser exposes it for the event.
    #[serde(default)]
    pub text: Option<SharedString>,
    /// HTML clipboard data when the browser exposes it for the event.
    #[serde(default)]
    pub html: Option<SharedString>,
    /// Whether the event target is inside an editable control or region.
    #[serde(default, rename = "targetEditable")]
    pub target_editable: bool,
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Whether page code had prevented default before Kael observed the event.
    #[serde(default, rename = "defaultPrevented")]
    pub default_prevented: bool,
}

impl WebViewClipboardEvent {
    /// Parse a bridge payload into a typed clipboard event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed clipboard event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// App decision for a WebView browser-permission preflight.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebViewPermissionDecision {
    /// Let the embedded browser continue with its normal permission flow.
    #[default]
    Default,
    /// Allow the page API call to continue to the embedded browser.
    Allow,
    /// Deny the page API call before it reaches the embedded browser.
    Deny,
}

impl WebViewPermissionDecision {
    fn as_bridge_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

/// Event payload emitted by [`webview_permission_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewPermissionRequest {
    /// Primary permission being requested, such as `"camera"` or `"geolocation"`.
    #[serde(default)]
    pub permission: SharedString,
    /// All permission facets involved in the request.
    #[serde(default)]
    pub permissions: Vec<SharedString>,
    /// Browser API that triggered the request.
    #[serde(default)]
    pub api: SharedString,
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Current document origin.
    #[serde(default)]
    pub origin: SharedString,
    /// Whether the browser reported transient user activation.
    #[serde(default, rename = "userGesture")]
    pub user_gesture: bool,
    /// JSON-serializable request details when the browser exposes them.
    #[serde(default)]
    pub details: serde_json::Value,
}

impl WebViewPermissionRequest {
    /// Parse a bridge payload into a typed permission request.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed permission request when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Event payload emitted by [`webview_storage_bridge_script`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebViewStorageEvent {
    /// Storage action such as `"setItem"`, `"removeItem"`, `"clear"`, or `"storage"`.
    #[serde(default)]
    pub event: SharedString,
    /// Storage area: `"localStorage"` or `"sessionStorage"`.
    #[serde(default)]
    pub area: SharedString,
    /// Storage key for key-specific changes.
    #[serde(default)]
    pub key: Option<SharedString>,
    /// Previous value when the browser exposes it.
    #[serde(default, rename = "oldValue")]
    pub old_value: Option<SharedString>,
    /// New value when the browser exposes it.
    #[serde(default, rename = "newValue")]
    pub new_value: Option<SharedString>,
    /// Number of keys in the storage area after the observed change.
    #[serde(default)]
    pub length: usize,
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Whether the event originated from the current document's wrapped API call.
    #[serde(default)]
    pub local: bool,
}

impl WebViewStorageEvent {
    /// Parse a bridge payload into a typed storage event.
    pub fn from_payload(payload: serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload).ok()
    }

    /// Parse a bridge message into a typed storage event when the kind matches.
    pub fn from_bridge_message(message: &WebViewBridgeMessage, kind: &str) -> Option<Self> {
        if message.is_kind(kind) {
            Self::from_payload(message.payload.clone())
        } else {
            None
        }
    }
}

/// Browser Web Storage area selected by WebView storage helpers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebViewStorageArea {
    /// Browser `localStorage`.
    #[default]
    Local,
    /// Browser `sessionStorage`.
    Session,
}

impl WebViewStorageArea {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "localStorage",
            Self::Session => "sessionStorage",
        }
    }
}

/// A key/value entry from a WebView storage snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewStorageEntry {
    /// Storage key.
    #[serde(default)]
    pub key: SharedString,
    /// Storage value.
    #[serde(default)]
    pub value: SharedString,
}

/// Snapshot of one browser Web Storage area.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewStorageAreaSnapshot {
    /// Storage area: `"localStorage"` or `"sessionStorage"`.
    #[serde(default)]
    pub area: SharedString,
    /// Whether the browser exposed this area to the current document.
    #[serde(default)]
    pub available: bool,
    /// Number of keys visible in this area.
    #[serde(default)]
    pub length: usize,
    /// Key/value entries in browser storage order.
    #[serde(default)]
    pub entries: Vec<WebViewStorageEntry>,
    /// Browser error text when the area could not be read.
    #[serde(default)]
    pub error: Option<SharedString>,
}

/// On-demand snapshot of browser Web Storage for a WebView document.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewStorageSnapshot {
    /// Current document URL.
    #[serde(default)]
    pub url: SharedString,
    /// Current document origin.
    #[serde(default)]
    pub origin: SharedString,
    /// Browser `localStorage` snapshot.
    #[serde(default, rename = "localStorage")]
    pub local_storage: WebViewStorageAreaSnapshot,
    /// Browser `sessionStorage` snapshot.
    #[serde(default, rename = "sessionStorage")]
    pub session_storage: WebViewStorageAreaSnapshot,
}

/// Result for a WebView Web Storage mutation command.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewStorageMutationResult {
    /// Whether the mutation completed.
    #[serde(default)]
    pub ok: bool,
    /// Storage area: `"localStorage"` or `"sessionStorage"`.
    #[serde(default)]
    pub area: SharedString,
    /// Mutated storage key when the command targets one key.
    #[serde(default)]
    pub key: Option<SharedString>,
    /// Number of keys in the storage area after the command, when readable.
    #[serde(default)]
    pub length: usize,
    /// Browser error text when the command failed.
    #[serde(default)]
    pub error: Option<SharedString>,
}

impl WebViewController {
    /// Create a controller for a WebView element id.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into().to_string().into(),
        }
    }

    /// Return the WebView element id this controller targets.
    pub fn id(&self) -> SharedString {
        self.id.clone()
    }

    /// Navigate the target WebView to a new URL.
    pub fn navigate(&self, window: &mut Window, url: impl Into<SharedString>) -> Result<()> {
        window.navigate_webview(self.id.clone(), url)
    }

    /// Navigate the target WebView to a new URL with additional request headers.
    pub fn navigate_with_headers(
        &self,
        window: &mut Window,
        url: impl Into<SharedString>,
        headers: http_client::http::HeaderMap,
    ) -> Result<()> {
        window.navigate_webview_with_headers(self.id.clone(), url, headers)
    }

    /// Load an HTML string into the target WebView.
    pub fn load_html(&self, window: &mut Window, html: impl Into<SharedString>) -> Result<()> {
        window.load_webview_html(self.id.clone(), html)
    }

    /// Evaluate JavaScript in the target WebView.
    pub fn evaluate_javascript(
        &self,
        window: &mut Window,
        script: impl Into<SharedString>,
    ) -> Result<()> {
        window.evaluate_webview_javascript(self.id.clone(), script)
    }

    /// Evaluate JavaScript in the target WebView and receive the serialized result.
    ///
    /// The returned value is the backend's JSON string serialization of the JavaScript result.
    pub fn evaluate_javascript_with_result(
        &self,
        window: &mut Window,
        script: impl Into<SharedString>,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        window.evaluate_webview_javascript_with_result(self.id.clone(), script, callback)
    }

    /// Insert or replace a named runtime CSS block in the target WebView.
    ///
    /// This mirrors browser-runtime `hosted CSS injection(...)` workflows for hosted
    /// widgets and browser-media islands, while using an app-chosen key so CSS
    /// can be updated or removed deterministically later.
    pub fn insert_css(&self, window: &mut Window, key: &str, css: &str) -> Result<()> {
        self.evaluate_javascript(window, webview_insert_css_script(key, css))
    }

    /// Remove a named runtime CSS block inserted with [`Self::insert_css`].
    pub fn remove_inserted_css(&self, window: &mut Window, key: &str) -> Result<()> {
        self.evaluate_javascript(window, webview_remove_inserted_css_script(key))
    }

    /// Find text in the target WebView and move browser selection to the match.
    ///
    /// The callback receives `Ok(true)` when the browser found and selected a
    /// match, `Ok(false)` when no match was found, or `Err(...)` when script
    /// execution failed.
    pub fn find_text(
        &self,
        window: &mut Window,
        query: impl Into<SharedString>,
        options: WebViewFindOptions,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_find_script(query.into().as_ref(), options);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Find text and return a richer result for native find bars.
    ///
    /// The browser still owns the active selection via `window.find(...)`, while
    /// Kael also counts DOM text matches in the current document so apps can
    /// show result counts without custom page JavaScript. Cross-origin frames
    /// and backend-native find match details are not included.
    pub fn find_text_result(
        &self,
        window: &mut Window,
        query: impl Into<SharedString>,
        options: WebViewFindOptions,
        callback: impl Fn(Result<WebViewFindResult, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let query = query.into();
        let script = webview_find_result_script(query.as_ref(), options);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_find_result(value.as_ref())));
        })
    }

    /// Clear the active browser find selection in the target WebView.
    pub fn stop_finding(&self, window: &mut Window) -> Result<()> {
        self.stop_finding_with_action(window, WebViewStopFindAction::ClearSelection)
    }

    /// Stop finding with an native desktop selection action.
    ///
    /// This mirrors `hosted find stop(action)`: `ClearSelection`
    /// removes the current browser selection, `KeepSelection` leaves it alone,
    /// and `ActivateSelection` focuses/scrolls the selected match where browser
    /// APIs allow it.
    pub fn stop_finding_with_action(
        &self,
        window: &mut Window,
        action: WebViewStopFindAction,
    ) -> Result<()> {
        self.evaluate_javascript(window, webview_stop_finding_script(action))
    }

    /// Execute a browser edit command in the target WebView.
    ///
    /// This covers common browser-runtime `hosted page controller` edit commands such as copy,
    /// cut, paste, select all, undo, and redo. The callback receives the
    /// browser's boolean `document.execCommand(...)` result.
    pub fn edit_command(
        &self,
        window: &mut Window,
        command: WebViewEditCommand,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_edit_command_script(command);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Insert text into the focused browser editor or form control.
    ///
    /// This mirrors browser-runtime `hosted page controller.insertText(...)` for command palettes,
    /// AI agents, test automation, and native editor chrome that need to type
    /// into hosted inputs or contenteditable documents without bespoke page
    /// JavaScript. The callback receives whether the browser accepted or
    /// emulated the insertion.
    pub fn insert_text(
        &self,
        window: &mut Window,
        text: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_insert_text_script(text.into().as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Focus the first element matching a CSS selector in the target WebView.
    ///
    /// This is a small browser-island automation helper for native chrome,
    /// tests, and AI agents. It avoids repeating raw `querySelector(...)`
    /// snippets when an app needs to focus a hosted input, editor, or control
    /// before sending edit commands.
    pub fn focus_selector(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_focus_selector_script(selector.into().as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Click the first element matching a CSS selector in the target WebView.
    ///
    /// This is useful for WebView-hosted buttons, links, tabs, and test fixtures
    /// where native code or an agent needs to trigger normal browser click
    /// behavior without custom page JavaScript.
    pub fn click_selector(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_click_selector_script(selector.into().as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Add a CSS class to the first element matching a selector.
    ///
    /// This is a small DOM customization helper for hosted widgets, context
    /// menus, tests, and agents. It uses normal `classList.add(...)` browser
    /// behavior and does not pierce cross-origin frames or shadow roots.
    pub fn add_class(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        class_name: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let class_name = class_name.into();
        let script =
            webview_class_action_script(selector.as_ref(), class_name.as_ref(), "add", None);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Remove a CSS class from the first element matching a selector.
    pub fn remove_class(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        class_name: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let class_name = class_name.into();
        let script =
            webview_class_action_script(selector.as_ref(), class_name.as_ref(), "remove", None);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Toggle a CSS class on the first element matching a selector.
    ///
    /// Pass `Some(true)` or `Some(false)` to force the final state, or `None`
    /// to invert the current class state.
    pub fn toggle_class(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        class_name: impl Into<SharedString>,
        force: Option<bool>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let class_name = class_name.into();
        let script =
            webview_class_action_script(selector.as_ref(), class_name.as_ref(), "toggle", force);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Set an attribute on the first element matching a selector.
    ///
    /// Use this for simple hosted-widget state such as `aria-*`, `data-*`,
    /// `hidden`, `src`, or `controls`. Page script and browser validation still
    /// own the final behavior of sensitive attributes.
    pub fn set_attribute(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        name: impl Into<SharedString>,
        value: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let name = name.into();
        let value = value.into();
        let script =
            webview_attribute_action_script(selector.as_ref(), name.as_ref(), Some(value.as_ref()));
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Remove an attribute from the first element matching a selector.
    pub fn remove_attribute(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        name: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let name = name.into();
        let script = webview_attribute_action_script(selector.as_ref(), name.as_ref(), None);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Set one inline CSS property on the first element matching a selector.
    ///
    /// The property name is passed to `style.setProperty(...)`, so CSS custom
    /// properties are allowed. This intentionally targets inline styles for
    /// narrow app/agent customization; use [`Self::insert_css`] for larger
    /// stylesheet-level changes.
    pub fn set_style_property(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        name: impl Into<SharedString>,
        value: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let name = name.into();
        let value = value.into();
        let script =
            webview_style_property_script(selector.as_ref(), name.as_ref(), Some(value.as_ref()));
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Remove one inline CSS property from the first element matching a selector.
    pub fn remove_style_property(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        name: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let name = name.into();
        let script = webview_style_property_script(selector.as_ref(), name.as_ref(), None);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Set the value of the first form control matching a CSS selector.
    ///
    /// This covers common hosted form automation for inputs, textareas,
    /// selects, checkboxes, radios, and contenteditable elements. It dispatches
    /// normal `input` and `change` events so page listeners can react as if the
    /// user edited the control.
    pub fn set_form_value(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        value: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let value = value.into();
        let script = webview_set_form_value_script(selector.as_ref(), value.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Submit the first form matching or containing a CSS selector.
    ///
    /// The selector may point at a `<form>` or at a control inside a form. Kael
    /// uses the browser's `requestSubmit()` path when available so normal
    /// validation and submit handlers run.
    pub fn submit_form(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_submit_form_script(selector.into().as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Reset the first form matching or containing a CSS selector.
    ///
    /// The selector may point at a `<form>` or at a control inside a form. Kael
    /// calls the browser's normal `form.reset()` path so reset events and
    /// default values are handled by the hosted document.
    pub fn reset_form(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_reset_form_script(selector.into().as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Copy the current browser selection in the target WebView.
    pub fn copy(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::Copy, callback)
    }

    /// Cut the current browser selection in the target WebView.
    pub fn cut(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::Cut, callback)
    }

    /// Paste into the focused editable browser element when the backend allows it.
    pub fn paste(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::Paste, callback)
    }

    /// Select all editable/browser document content in the target WebView.
    pub fn select_all(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::SelectAll, callback)
    }

    /// Undo the last browser editing action in the target WebView.
    pub fn undo(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::Undo, callback)
    }

    /// Redo the last undone browser editing action in the target WebView.
    pub fn redo(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::Redo, callback)
    }

    /// Delete the current browser selection in the target WebView.
    pub fn delete_selection(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.edit_command(window, WebViewEditCommand::Delete, callback)
    }

    /// Read the current browser selection as text.
    ///
    /// This mirrors browser-runtime `hosted selected-text query` for context menus,
    /// inspectors, find bars, and hosted editor chrome. It handles both normal
    /// document selections and focused `<input>` / `<textarea>` selection ranges.
    pub fn selected_text(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_selected_text_script(),
            move |result| {
                callback(result.and_then(|value| parse_webview_string_result(value.as_ref())));
            },
        )
    }

    /// Read the current browser selection as HTML.
    ///
    /// This is useful for rich-editor context menus, inspectors, and export
    /// flows. Normal document selections are serialized from cloned selection
    /// ranges; focused `<input>` / `<textarea>` selections are returned as
    /// escaped text because those controls do not expose rich HTML fragments.
    pub fn selected_html(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_selected_html_script(),
            move |result| {
                callback(result.and_then(|value| parse_webview_string_result(value.as_ref())));
            },
        )
    }

    /// Read the current document element as serialized HTML.
    ///
    /// This mirrors the common browser-runtime stack pattern of calling
    /// `executeJavaScript("document.documentElement.outerHTML")` for page
    /// inspectors, export flows, bug reports, and AI-agent page understanding.
    /// Cross-origin frames remain owned by the browser engine and are not
    /// expanded into this string.
    pub fn document_html(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_document_html_script(),
            move |result| {
                callback(result.and_then(|value| parse_webview_string_result(value.as_ref())));
            },
        )
    }

    /// Read a structured snapshot of the current document.
    ///
    /// This is intended for diagnostics, tests, page inspectors, and AI-agent
    /// page understanding. It captures same-origin top-document metadata,
    /// visible text, headings, links, images, and forms without expanding
    /// cross-origin frames or fetching resource bytes.
    pub fn document_snapshot(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<WebViewDocumentSnapshot, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_document_snapshot_script(),
            move |result| {
                callback(
                    result.and_then(|value| parse_webview_document_snapshot_result(value.as_ref())),
                );
            },
        )
    }

    /// Read a structured snapshot for the first element matching a CSS selector.
    ///
    /// This is a narrower companion to [`Self::document_snapshot`] for native
    /// inspectors, tests, and AI agents that need to decide how to interact
    /// with a hosted control before calling selector-scoped mutation helpers.
    /// It only inspects the current top document; cross-origin frames and
    /// shadow roots remain owned by the browser engine.
    pub fn element_snapshot(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        callback: impl Fn(Result<Option<WebViewElementSnapshot>, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let script = webview_element_snapshot_script(selector.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(
                result.and_then(|value| parse_webview_element_snapshot_result(value.as_ref())),
            );
        })
    }

    /// Capture a same-document DOM element as an SVG data URL.
    ///
    /// This is a lightweight thumbnail/preview helper for app-owned hosted
    /// widgets. It clones the selected element, inlines computed styles, and
    /// wraps the clone in an SVG `foreignObject`. It is not a native pixel
    /// screenshot, does not pierce cross-origin frames or shadow roots, and
    /// browser media, canvas, WebGL, and external resources may not serialize
    /// with visual fidelity.
    pub fn capture_dom_image(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        options: WebViewDomImageCaptureOptions,
        callback: impl Fn(Result<Option<SharedString>, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let script = webview_capture_dom_image_script(selector.as_ref(), &options);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_optional_string_result(value.as_ref())));
        })
    }

    /// Ask the hosted document to trigger a browser download for a URL.
    ///
    /// This is designed for native context menus and agents that receive a
    /// `linkHref`, `imageSrc`, or `mediaSrc` from Kael's WebView bridges and
    /// need a "Save..." command without taking over networking. The URL is
    /// resolved against the current document and the filename is passed as the
    /// browser `<a download>` hint. Browser origin rules, response headers,
    /// and Kael's download policy handlers still decide whether and where the
    /// download actually completes.
    pub fn trigger_download(
        &self,
        window: &mut Window,
        url: impl Into<SharedString>,
        filename: Option<impl Into<SharedString>>,
        callback: impl Fn(Result<WebViewDownloadTriggerResult, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let url = url.into();
        let filename = filename.map(Into::into);
        let script = webview_trigger_download_script(url.as_ref(), filename.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(
                result.and_then(|value| parse_webview_download_trigger_result(value.as_ref())),
            );
        })
    }

    /// Alias for [`WebViewController::trigger_download`].
    pub fn download_url(
        &self,
        window: &mut Window,
        url: impl Into<SharedString>,
        filename: Option<impl Into<SharedString>>,
        callback: impl Fn(Result<WebViewDownloadTriggerResult, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.trigger_download(window, url, filename, callback)
    }

    /// Read favicon candidates from the current document.
    ///
    /// This mirrors native desktop tab chrome that reacts to hosted page icons.
    /// It returns resolved URLs from `<link rel="icon">`, shortcut icons,
    /// Apple touch icons, and mask icons in document order. It does not fetch
    /// or decode image bytes.
    pub fn favicons(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<Vec<SharedString>, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(window, webview_favicons_script(), move |result| {
            callback(result.and_then(|value| parse_webview_string_array_result(value.as_ref())));
        })
    }

    /// Read the current `document.title` from the target WebView.
    ///
    /// This mirrors browser-runtime `hosted title query` for tab labels,
    /// breadcrumbs, inspectors, and restore flows that need the title on
    /// demand rather than only through title-change events.
    pub fn title(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(window, "document.title", move |result| {
            callback(result.and_then(|value| parse_webview_string_result(value.as_ref())));
        })
    }

    /// Read the effective browser user agent from the target WebView.
    ///
    /// This mirrors browser-runtime `hosted user-agent query` for diagnostics,
    /// hosted service compatibility checks, and verifying custom
    /// [`WebViewOptions::user_agent`] configuration.
    pub fn user_agent(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(window, "navigator.userAgent", move |result| {
            callback(result.and_then(|value| parse_webview_string_result(value.as_ref())));
        })
    }

    /// Read whether the target WebView document is still loading.
    ///
    /// This mirrors the common browser-runtime `hosted loading query` workflow for
    /// app-owned loading indicators and route guards. It is based on
    /// `document.readyState !== "complete"` rather than backend-native network
    /// activity counters.
    pub fn is_loading(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(window, webview_is_loading_script(), move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Read whether the target WebView likely has a previous history entry.
    ///
    /// This mirrors the common browser-runtime `hosted back-state query` workflow for
    /// native Back buttons. It uses the browser History API
    /// (`history.length > 1`), which is portable but less precise than
    /// backend-native navigation-stack state.
    pub fn can_go_back(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(window, webview_can_go_back_script(), move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Read whether the target WebView likely has a forward history entry.
    ///
    /// This mirrors the common browser-runtime `hosted forward-state query` workflow
    /// for native Forward buttons. Browser JavaScript cannot inspect the
    /// backend forward stack directly, so this reads an app/page-provided
    /// `window.__kaelNavigationState.canGoForward` marker when present and
    /// otherwise returns `false` conservatively. Use
    /// [`WebViewOptions::navigation_state_bridge`] or
    /// [`WebView::navigation_state_bridge`] for app-owned pages that need a
    /// portable Forward button before native backend stack reads are exposed.
    pub fn can_go_forward(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_can_go_forward_script(),
            move |result| {
                callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
            },
        )
    }

    /// Read the current top-document viewport and scroll state.
    pub fn viewport_snapshot(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<WebViewScrollEvent, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_viewport_snapshot_script("snapshot"),
            move |result| {
                callback(
                    result.and_then(|value| parse_webview_scroll_event_result(value.as_ref())),
                );
            },
        )
    }

    /// Scroll the current top document to an absolute position in CSS pixels.
    pub fn scroll_to(
        &self,
        window: &mut Window,
        x: f64,
        y: f64,
        callback: impl Fn(Result<WebViewScrollEvent, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_scroll_to_script(x, y),
            move |result| {
                callback(
                    result.and_then(|value| parse_webview_scroll_event_result(value.as_ref())),
                );
            },
        )
    }

    /// Scroll the current top document by a relative delta in CSS pixels.
    pub fn scroll_by(
        &self,
        window: &mut Window,
        delta_x: f64,
        delta_y: f64,
        callback: impl Fn(Result<WebViewScrollEvent, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_scroll_by_script(delta_x, delta_y),
            move |result| {
                callback(
                    result.and_then(|value| parse_webview_scroll_event_result(value.as_ref())),
                );
            },
        )
    }

    /// Scroll the first matching top-document element into view.
    ///
    /// Returns `Ok(None)` when the selector is invalid or no element matches.
    /// Shadow roots and cross-origin frames remain browser-owned.
    pub fn scroll_selector_into_view(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        callback: impl Fn(Result<Option<WebViewScrollEvent>, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        self.evaluate_javascript_with_result(
            window,
            webview_scroll_selector_into_view_script(selector.as_ref()),
            move |result| {
                callback(
                    result.and_then(|value| {
                        parse_webview_optional_scroll_event_result(value.as_ref())
                    }),
                );
            },
        )
    }

    /// Post a structured message into the target WebView.
    pub fn post_message(&self, window: &mut Window, message: serde_json::Value) -> Result<()> {
        window.post_webview_message(self.id.clone(), message)
    }

    /// Post a typed bridge envelope into the target WebView.
    pub fn post_bridge_message(
        &self,
        window: &mut Window,
        message: impl Into<WebViewBridgeMessage>,
    ) -> Result<()> {
        self.post_message(window, serde_json::to_value(message.into())?)
    }

    /// Respond to a JavaScript `window.kael.invoke(...)` request.
    pub fn respond_to_bridge_message(
        &self,
        window: &mut Window,
        request: &WebViewBridgeMessage,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.post_bridge_message(window, WebViewBridgeMessage::response_to(request, payload))
    }

    /// Reject a JavaScript `window.kael.invoke(...)` request.
    pub fn reject_bridge_message(
        &self,
        window: &mut Window,
        request: &WebViewBridgeMessage,
        message: impl Into<String>,
    ) -> Result<()> {
        self.post_bridge_message(window, WebViewBridgeMessage::error_to(request, message))
    }

    /// Reload the target WebView.
    pub fn reload(&self, window: &mut Window) -> Result<()> {
        window.reload_webview(self.id.clone())
    }

    /// Stop loading resources in the target WebView.
    ///
    /// This mirrors browser-runtime `hosted load stop` through the browser's
    /// standard `window.stop()` primitive so it works across supported
    /// WebView backends.
    pub fn stop_loading(&self, window: &mut Window) -> Result<()> {
        window.stop_loading_webview(self.id.clone())
    }

    /// Play every browser media element in the target WebView.
    ///
    /// Browser autoplay and user-gesture policies still apply. Rejected
    /// `play()` promises are intentionally swallowed so a single blocked or
    /// unsupported element does not break the rest of the page script.
    pub fn play_media(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_play_media_script())
    }

    /// Pause every browser media element in the target WebView.
    ///
    /// This is useful for WebView-hosted video/audio fallbacks, docs widgets,
    /// calls, and other browser-media islands when the native app needs to
    /// pause playback during navigation, window hiding, or route changes.
    pub fn pause_media(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_pause_media_script())
    }

    /// Mute or unmute every browser media element in the target WebView.
    ///
    /// This changes the `muted` property on current `<audio>` and `<video>`
    /// elements. New media elements created by the page should still be managed
    /// by page code or an injected script.
    pub fn set_media_muted(&self, window: &mut Window, muted: bool) -> Result<()> {
        self.evaluate_javascript(window, webview_set_media_muted_script(muted))
    }

    /// Set the volume on every browser media element in the target WebView.
    ///
    /// Values are clamped to the browser media element range of `0.0..=1.0`.
    pub fn set_media_volume(&self, window: &mut Window, volume: f32) -> Result<()> {
        self.evaluate_javascript(window, webview_set_media_volume_script(volume))
    }

    /// Set the playback rate on every browser media element in the target WebView.
    ///
    /// Values below zero are clamped to `0.0`; individual browsers may still
    /// reject rates outside their supported media playback range.
    pub fn set_media_playback_rate(&self, window: &mut Window, rate: f32) -> Result<()> {
        self.evaluate_javascript(window, webview_set_media_playback_rate_script(rate))
    }

    /// Seek every browser media element in the target WebView to a time in seconds.
    pub fn seek_media_secs(&self, window: &mut Window, seconds: f64) -> Result<()> {
        self.evaluate_javascript(window, webview_seek_media_secs_script(seconds))
    }

    /// Run a media command on the first matching browser media element.
    ///
    /// The selector may point at an `<audio>`, `<video>`, or descendant element.
    /// This is the selector-scoped counterpart to broad helpers such as
    /// [`Self::play_media`] and [`Self::pause_media`].
    pub fn media_command(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        command: WebViewMediaCommand,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let script = webview_media_command_script(selector.as_ref(), command);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Set the source URL for the first matching browser media element.
    ///
    /// The selector may point at an `<audio>`, `<video>`, or nested `<source>`
    /// element. Kael updates the element's `src` and calls the browser's normal
    /// `load()` path so metadata, buffering, and media events are owned by the
    /// embedded engine.
    pub fn set_media_source(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        source: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let source = source.into();
        let script = webview_set_media_source_script(selector.as_ref(), source.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Apply common browser media properties to the first matching element.
    ///
    /// The selector may point at an `<audio>`, `<video>`, or descendant element.
    /// Kael sets normal media playback surface properties/attributes such as controls,
    /// loop, autoplay, muted, playsinline, poster, preload, controlslist, and
    /// disablePictureInPicture where the browser supports them.
    pub fn set_media_options(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        options: WebViewMediaElementOptions,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let script = webview_set_media_options_script(selector.as_ref(), &options);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Capture the current frame from the first matching browser video element.
    ///
    /// The selector may point at a `<video>` or descendant element. Kael draws
    /// the current video frame into a canvas and returns a data URL. Browser
    /// CORS/tainted-canvas rules still apply; unavailable or uncapturable frames
    /// return `Ok(None)`.
    pub fn capture_media_frame(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        options: WebViewMediaFrameCaptureOptions,
        callback: impl Fn(Result<Option<SharedString>, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let script = webview_capture_media_frame_script(selector.as_ref(), &options);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_optional_string_result(value.as_ref())));
        })
    }

    /// Add a browser text track to the first matching media element.
    ///
    /// The selector may point at an `<audio>`, `<video>`, or descendant element.
    /// Kael appends a real `<track>` child, so the embedded browser owns WebVTT
    /// loading, cue parsing, and TextTrack state.
    pub fn add_media_text_track(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        track: WebViewMediaTextTrackOptions,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let script = webview_add_media_text_track_script(selector.as_ref(), &track);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Remove matching browser text-track elements from the first matching media element.
    ///
    /// The media selector may point at an `<audio>`, `<video>`, or descendant
    /// element. The track selector matches a track element's id, label, srclang,
    /// kind, src, or zero-based index.
    pub fn remove_media_text_track(
        &self,
        window: &mut Window,
        selector: impl Into<SharedString>,
        track_selector: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let selector = selector.into();
        let track_selector = track_selector.into();
        let script =
            webview_remove_media_text_track_script(selector.as_ref(), track_selector.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(result.and_then(|value| parse_webview_bool_result(value.as_ref())));
        })
    }

    /// Select matching browser text tracks and disable the rest.
    ///
    /// The selector matches a track's id, label, language, or zero-based index
    /// string across all current WebView `<audio>` and `<video>` elements.
    pub fn select_media_text_track(&self, window: &mut Window, selector: &str) -> Result<()> {
        self.evaluate_javascript(window, webview_select_media_text_track_script(selector))
    }

    /// Disable all browser text tracks on current WebView media elements.
    pub fn disable_media_text_tracks(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_disable_media_text_tracks_script())
    }

    /// Request browser fullscreen for the first available video or media element.
    ///
    /// Browser user-gesture and embedding policies still apply. Rejected
    /// fullscreen promises are swallowed so unsupported pages do not break app
    /// script execution.
    pub fn request_media_fullscreen(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_request_media_fullscreen_script())
    }

    /// Exit browser fullscreen when the WebView document is fullscreen.
    pub fn exit_media_fullscreen(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_exit_media_fullscreen_script())
    }

    /// Request picture-in-picture for the first video element that supports it.
    ///
    /// Browser support, page attributes, permissions, and user-gesture policies
    /// still apply. Rejected promises are swallowed.
    pub fn request_media_picture_in_picture(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_request_media_picture_in_picture_script())
    }

    /// Exit browser picture-in-picture when an element is currently active.
    pub fn exit_media_picture_in_picture(&self, window: &mut Window) -> Result<()> {
        self.evaluate_javascript(window, webview_exit_media_picture_in_picture_script())
    }

    /// Read state for every browser media element in the target WebView.
    ///
    /// This snapshots current `<audio>` and `<video>` elements so native chrome
    /// can drive WebView-hosted players without hand-written state scraping.
    pub fn media_state(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<Vec<WebViewMediaElementState>, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(window, webview_media_state_script(), move |result| {
            callback(result.and_then(|value| parse_webview_media_state_result(value.as_ref())));
        })
    }

    /// Mute every browser media element in the target WebView.
    pub fn mute_media(&self, window: &mut Window) -> Result<()> {
        self.set_media_muted(window, true)
    }

    /// Unmute every browser media element in the target WebView.
    pub fn unmute_media(&self, window: &mut Window) -> Result<()> {
        self.set_media_muted(window, false)
    }

    /// Navigate the target WebView backward if possible.
    pub fn go_back(&self, window: &mut Window) -> Result<()> {
        window.go_back_webview(self.id.clone())
    }

    /// Navigate the target WebView forward if possible.
    pub fn go_forward(&self, window: &mut Window) -> Result<()> {
        window.go_forward_webview(self.id.clone())
    }

    /// Open WebView developer tools when the active backend supports it.
    ///
    /// Devtools are available in debug builds on Wry-backed WebViews. Release
    /// builds require a backend/devtools feature that may not be enabled.
    pub fn open_devtools(&self, window: &mut Window) -> Result<()> {
        window.open_webview_devtools(self.id.clone())
    }

    /// Close WebView developer tools when the active backend supports it.
    pub fn close_devtools(&self, window: &mut Window) -> Result<()> {
        window.close_webview_devtools(self.id.clone())
    }

    /// Read whether WebView developer tools are open when the active backend supports it.
    pub fn is_devtools_open(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<bool, SharedString>) + 'static,
    ) -> Result<()> {
        window.is_webview_devtools_open(self.id.clone(), callback)
    }

    /// Open the platform print dialog for the target WebView content.
    pub fn print(&self, window: &mut Window) -> Result<()> {
        window.print_webview(self.id.clone())
    }

    /// Set the target WebView's browser zoom factor.
    pub fn set_zoom_factor(&self, window: &mut Window, factor: f64) -> Result<()> {
        window.set_webview_zoom_factor(self.id.clone(), factor)
    }

    /// Move focus into the target WebView when the active backend supports it.
    pub fn focus(&self, window: &mut Window) -> Result<()> {
        window.focus_webview(self.id.clone())
    }

    /// Move focus from the target WebView back to the parent window.
    pub fn focus_parent(&self, window: &mut Window) -> Result<()> {
        window.focus_webview_parent(self.id.clone())
    }

    /// Clear cookies, cache, local storage, and other browsing data for this WebView profile.
    pub fn clear_browsing_data(&self, window: &mut Window) -> Result<()> {
        window.clear_webview_browsing_data(self.id.clone())
    }

    /// Read `localStorage` and `sessionStorage` from the current document.
    ///
    /// This is an on-demand companion to [`WebViewOptions::storage_bridge`].
    /// Browser origin/security rules still apply; blocked areas are returned
    /// with `available: false` and an error string instead of being treated as
    /// a transport failure.
    pub fn storage_snapshot(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<WebViewStorageSnapshot, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.evaluate_javascript_with_result(
            window,
            webview_storage_snapshot_script(),
            move |result| {
                callback(
                    result.and_then(|value| parse_webview_storage_snapshot_result(value.as_ref())),
                );
            },
        )
    }

    /// Set one key in the current document's browser Web Storage.
    pub fn set_storage_item(
        &self,
        window: &mut Window,
        area: WebViewStorageArea,
        key: impl Into<SharedString>,
        value: impl Into<SharedString>,
        callback: impl Fn(Result<WebViewStorageMutationResult, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let key = key.into();
        let value = value.into();
        let script = webview_set_storage_item_script(area, key.as_ref(), value.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(
                result.and_then(|value| parse_webview_storage_mutation_result(value.as_ref())),
            );
        })
    }

    /// Remove one key from the current document's browser Web Storage.
    pub fn remove_storage_item(
        &self,
        window: &mut Window,
        area: WebViewStorageArea,
        key: impl Into<SharedString>,
        callback: impl Fn(Result<WebViewStorageMutationResult, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let key = key.into();
        let script = webview_remove_storage_item_script(area, key.as_ref());
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(
                result.and_then(|value| parse_webview_storage_mutation_result(value.as_ref())),
            );
        })
    }

    /// Clear one current-document browser Web Storage area.
    pub fn clear_storage_area(
        &self,
        window: &mut Window,
        area: WebViewStorageArea,
        callback: impl Fn(Result<WebViewStorageMutationResult, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        let script = webview_clear_storage_area_script(area);
        self.evaluate_javascript_with_result(window, script, move |result| {
            callback(
                result.and_then(|value| parse_webview_storage_mutation_result(value.as_ref())),
            );
        })
    }

    /// Read the current URL reported by this WebView.
    pub fn url(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<SharedString, SharedString>) + 'static,
    ) -> Result<()> {
        window.read_webview_url(self.id.clone(), callback)
    }

    /// Read all cookies visible to this WebView.
    pub fn cookies(
        &self,
        window: &mut Window,
        callback: impl Fn(Result<Vec<WebViewCookie>, SharedString>) + 'static,
    ) -> Result<()> {
        window.read_webview_cookies(self.id.clone(), callback)
    }

    /// Read cookies for a URL from this WebView.
    pub fn cookies_for_url(
        &self,
        window: &mut Window,
        url: impl Into<SharedString>,
        callback: impl Fn(Result<Vec<WebViewCookie>, SharedString>) + 'static,
    ) -> Result<()> {
        window.read_webview_cookies_for_url(self.id.clone(), url, callback)
    }

    /// Set a cookie in this WebView profile.
    pub fn set_cookie(
        &self,
        window: &mut Window,
        cookie: WebViewCookie,
        callback: impl Fn(Result<(), SharedString>) + 'static,
    ) -> Result<()> {
        window.set_webview_cookie(self.id.clone(), cookie, callback)
    }

    /// Delete a cookie from this WebView profile.
    pub fn delete_cookie(
        &self,
        window: &mut Window,
        cookie: WebViewCookie,
        callback: impl Fn(Result<(), SharedString>) + 'static,
    ) -> Result<()> {
        window.delete_webview_cookie(self.id.clone(), cookie, callback)
    }
}

/// A small, native desktop envelope for messages crossing a WebView island.
///
/// JavaScript can send the same shape with `window.kael.post(kind, payload, id)`;
/// Rust can send it with [`WebViewController::post_bridge_message`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebViewBridgeMessage {
    /// The application-defined message kind, such as `"ready"` or `"select-file"`.
    pub kind: SharedString,
    /// Optional request/correlation id for request/response flows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<SharedString>,
    /// Application-defined JSON payload.
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl WebViewBridgeMessage {
    /// Create a bridge message with a kind and empty payload.
    pub fn new(kind: impl Into<SharedString>) -> Self {
        Self {
            kind: kind.into(),
            id: None,
            payload: serde_json::Value::Null,
        }
    }

    /// Create a bridge message with a kind and JSON payload.
    pub fn with_payload(kind: impl Into<SharedString>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            id: None,
            payload,
        }
    }

    /// Set a request/correlation id.
    pub fn with_id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Build a response envelope for an incoming request.
    ///
    /// The response keeps the request id and uses `{request.kind}:response`
    /// as its kind so JavaScript `window.kael.invoke(...)` can resolve the
    /// matching promise.
    pub fn response_to(request: &Self, payload: serde_json::Value) -> Self {
        Self {
            kind: format!("{}:response", request.kind).into(),
            id: request.id.clone(),
            payload,
        }
    }

    /// Build an error envelope for an incoming request.
    ///
    /// JavaScript `window.kael.invoke(...)` rejects the matching promise when
    /// it receives this `{request.kind}:error` envelope with the same id.
    pub fn error_to(request: &Self, message: impl Into<String>) -> Self {
        Self {
            kind: format!("{}:error", request.kind).into(),
            id: request.id.clone(),
            payload: serde_json::json!({ "message": message.into() }),
        }
    }

    /// Return true when this message kind matches the given string.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind.as_ref() == kind
    }

    /// Parse an arbitrary JSON value into a bridge message.
    pub fn from_value(value: serde_json::Value) -> Option<Self> {
        serde_json::from_value(value).ok()
    }

    /// Convert this bridge message into a JSON value.
    pub fn into_value(self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl From<WebViewBridgeMessage> for serde_json::Value {
    fn from(message: WebViewBridgeMessage) -> Self {
        message.into_value()
    }
}

fn webview_find_script(query: &str, options: WebViewFindOptions) -> SharedString {
    let query = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(() => {{ if (typeof window.find !== 'function') return false; return !!window.find({query}, {}, {}, {}, {}, {}, false); }})()",
        options.case_sensitive,
        options.backwards,
        options.wrap,
        options.whole_word,
        options.search_in_frames,
    )
    .into()
}

fn webview_find_result_script(query: &str, options: WebViewFindOptions) -> SharedString {
    let query = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const query = {query};
  const caseSensitive = {};
  const backwards = {};
  const wrap = {};
  const wholeWord = {};
  const searchInFrames = {};
  const textQuery = String(query || "");
  if (!textQuery) return {{ found: false, matches: 0 }};
  const haystackNeedle = caseSensitive ? textQuery : textQuery.toLocaleLowerCase();
  const isWord = (char) => !!char && /[\p{{L}}\p{{N}}_]/u.test(char);
  const countInText = (value) => {{
    const text = caseSensitive ? value : value.toLocaleLowerCase();
    let count = 0;
    let from = 0;
    while (from <= text.length) {{
      const index = text.indexOf(haystackNeedle, from);
      if (index === -1) break;
      const before = value[index - 1] || "";
      const after = value[index + textQuery.length] || "";
      if (!wholeWord || (!isWord(before) && !isWord(after))) count += 1;
      from = index + Math.max(textQuery.length, 1);
    }}
    return count;
  }};
  let matches = 0;
  const root = document.body || document.documentElement;
  if (root && document.createTreeWalker) {{
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {{
      acceptNode(node) {{
        const tag = node.parentElement && node.parentElement.tagName;
        if (tag && /^(SCRIPT|STYLE|NOSCRIPT|TEXTAREA|INPUT)$/i.test(tag)) {{
          return NodeFilter.FILTER_REJECT;
        }}
        return node.nodeValue ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
      }}
    }});
    while (walker.nextNode()) matches += countInText(walker.currentNode.nodeValue || "");
  }}
  const found = typeof window.find === "function"
    ? !!window.find(textQuery, caseSensitive, backwards, wrap, wholeWord, searchInFrames, false)
    : matches > 0;
  return {{ found, matches }};
}})()"#,
        options.case_sensitive,
        options.backwards,
        options.wrap,
        options.whole_word,
        options.search_in_frames,
    )
    .into()
}

fn webview_find_result_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"find:result\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelFindResultBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const originalFind = typeof window.find === "function" ? window.find.bind(window) : null;
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const isWord = (char) => !!char && /[\p{{L}}\p{{N}}_]/u.test(char);
  const countMatches = (query, caseSensitive, wholeWord) => {{
    const textQuery = String(query || "");
    if (!textQuery) return 0;
    const needle = caseSensitive ? textQuery : textQuery.toLocaleLowerCase();
    const countInText = (value) => {{
      const text = caseSensitive ? value : value.toLocaleLowerCase();
      let count = 0;
      let from = 0;
      while (from <= text.length) {{
        const index = text.indexOf(needle, from);
        if (index === -1) break;
        const before = value[index - 1] || "";
        const after = value[index + textQuery.length] || "";
        if (!wholeWord || (!isWord(before) && !isWord(after))) count += 1;
        from = index + Math.max(textQuery.length, 1);
      }}
      return count;
    }};
    let matches = 0;
    const root = document.body || document.documentElement;
    if (root && document.createTreeWalker) {{
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {{
        acceptNode(node) {{
          const tag = node.parentElement && node.parentElement.tagName;
          if (tag && /^(SCRIPT|STYLE|NOSCRIPT|TEXTAREA|INPUT)$/i.test(tag)) {{
            return NodeFilter.FILTER_REJECT;
          }}
          return node.nodeValue ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
        }}
      }});
      while (walker.nextNode()) matches += countInText(walker.currentNode.nodeValue || "");
    }}
    return matches;
  }};
  window.find = function(query, caseSensitive, backwards, wrap, wholeWord, searchInFrames, showDialog) {{
    const found = originalFind
      ? !!originalFind(query, !!caseSensitive, !!backwards, !!wrap, !!wholeWord, !!searchInFrames, !!showDialog)
      : countMatches(query, !!caseSensitive, !!wholeWord) > 0;
    let selectionText = "";
    try {{
      selectionText = String((window.getSelection && window.getSelection()) || "");
    }} catch (_) {{}}
    post({{
      event: "find",
      query: String(query || ""),
      found,
      matches: countMatches(query, !!caseSensitive, !!wholeWord),
      caseSensitive: !!caseSensitive,
      backwards: !!backwards,
      wrap: !!wrap,
      wholeWord: !!wholeWord,
      searchInFrames: !!searchInFrames,
      selectionText,
      url: String(location.href || ""),
    }});
    return found;
  }};
}})();"#
    )
    .into()
}

fn webview_stop_finding_script(action: WebViewStopFindAction) -> SharedString {
    let action = match action {
        WebViewStopFindAction::ClearSelection => "clearSelection",
        WebViewStopFindAction::KeepSelection => "keepSelection",
        WebViewStopFindAction::ActivateSelection => "activateSelection",
    };
    format!(
        r#"(() => {{
  const action = "{}";
  const selection = window.getSelection && window.getSelection();
  if (action === "activateSelection") {{
    if (selection && selection.rangeCount > 0) {{
      const range = selection.getRangeAt(0);
      const target = range.startContainer && (range.startContainer.nodeType === Node.ELEMENT_NODE
        ? range.startContainer
        : range.startContainer.parentElement);
      if (target && typeof target.scrollIntoView === "function") {{
        target.scrollIntoView({{ block: "nearest", inline: "nearest" }});
      }}
      if (target && typeof target.focus === "function") target.focus();
    }}
    return;
  }}
  if (action === "clearSelection") {{
    if (selection) selection.removeAllRanges();
    if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
  }}
}})();"#,
        action
    )
    .into()
}

fn webview_edit_command_script(command: WebViewEditCommand) -> SharedString {
    let command = command.document_command();
    format!(
        "(() => {{ if (!document || typeof document.execCommand !== 'function') return false; return !!document.execCommand({}); }})()",
        serde_json::to_string(command).unwrap_or_else(|_| "\"\"".into())
    )
    .into()
}

fn webview_insert_text_script(text: &str) -> SharedString {
    let text = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const text = {text};
  const emitInput = (target) => {{
    let event;
    try {{
      event = new InputEvent("input", {{ bubbles: true, inputType: "insertText", data: text }});
    }} catch (_) {{
      event = new Event("input", {{ bubbles: true }});
    }}
    target.dispatchEvent(event);
  }};
  if (document && typeof document.execCommand === "function") {{
    try {{
      if (document.execCommand("insertText", false, text)) return true;
    }} catch (_) {{}}
  }}
  const active = document.activeElement;
  if (active && typeof active.value === "string" &&
      typeof active.selectionStart === "number" &&
      typeof active.selectionEnd === "number" &&
      !active.disabled && !active.readOnly) {{
    const start = active.selectionStart || 0;
    const end = active.selectionEnd || start;
    if (typeof active.setRangeText === "function") {{
      active.setRangeText(text, start, end, "end");
    }} else {{
      active.value = active.value.slice(0, start) + text + active.value.slice(end);
      const cursor = start + text.length;
      if (typeof active.setSelectionRange === "function") active.setSelectionRange(cursor, cursor);
    }}
    emitInput(active);
    return true;
  }}
  if (active && active.isContentEditable && window.getSelection) {{
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0) return false;
    const range = selection.getRangeAt(0);
    range.deleteContents();
    const node = document.createTextNode(text);
    range.insertNode(node);
    range.setStartAfter(node);
    range.setEndAfter(node);
    selection.removeAllRanges();
    selection.addRange(range);
    emitInput(active);
    return true;
  }}
  return false;
}})()"#
    )
    .into()
}

fn webview_focus_selector_script(selector: &str) -> SharedString {
    webview_selector_action_script(selector, "focus")
}

fn webview_click_selector_script(selector: &str) -> SharedString {
    webview_selector_action_script(selector, "click")
}

fn webview_class_action_script(
    selector: &str,
    class_name: &str,
    action: &str,
    force: Option<bool>,
) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let class_name = serde_json::to_string(class_name).unwrap_or_else(|_| "\"\"".into());
    let action = serde_json::to_string(action).unwrap_or_else(|_| "\"add\"".into());
    let force = serde_json::to_string(&force).unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const className = {class_name};
  const action = {action};
  const force = {force};
  let element = null;
  try {{
    element = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!element || !element.classList || !className) return false;
  if (action === "add") {{
    element.classList.add(className);
    return true;
  }}
  if (action === "remove") {{
    element.classList.remove(className);
    return true;
  }}
  if (action === "toggle") {{
    if (force === true || force === false) {{
      return !!element.classList.toggle(className, force);
    }}
    return !!element.classList.toggle(className);
  }}
  return false;
}})()"#
    )
    .into()
}

fn webview_attribute_action_script(
    selector: &str,
    name: &str,
    value: Option<&str>,
) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let name = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
    let value = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const name = {name};
  const value = {value};
  let element = null;
  try {{
    element = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!element || !name || typeof element.setAttribute !== "function") return false;
  try {{
    if (value == null) {{
      element.removeAttribute(name);
    }} else {{
      element.setAttribute(name, String(value));
    }}
    return true;
  }} catch (_) {{
    return false;
  }}
}})()"#
    )
    .into()
}

fn webview_style_property_script(selector: &str, name: &str, value: Option<&str>) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let name = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
    let value = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const name = {name};
  const value = {value};
  let element = null;
  try {{
    element = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!element || !element.style || !name) return false;
  try {{
    if (value == null) {{
      element.style.removeProperty(name);
    }} else {{
      element.style.setProperty(name, String(value));
    }}
    return true;
  }} catch (_) {{
    return false;
  }}
}})()"#
    )
    .into()
}

fn webview_set_form_value_script(selector: &str, value: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let value = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const value = {value};
  const emit = (target, name) => {{
    let event;
    try {{
      event = new Event(name, {{ bubbles: true }});
    }} catch (_) {{
      event = document.createEvent("Event");
      event.initEvent(name, true, false);
    }}
    target.dispatchEvent(event);
  }};
  let element = null;
  try {{
    element = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!element || element.disabled || element.readOnly) return false;
  const tag = String(element.tagName || "").toLowerCase();
  const type = String(element.type || "").toLowerCase();
  if (tag === "select") {{
    let matched = false;
    for (const option of Array.from(element.options || [])) {{
      const selected = option.value === value || option.text === value;
      if (selected) matched = true;
      if (element.multiple) {{
        option.selected = selected;
      }} else if (selected) {{
        element.value = option.value;
        break;
      }}
    }}
    if (!matched) element.value = value;
    emit(element, "input");
    emit(element, "change");
    return true;
  }}
  if (type === "checkbox" || type === "radio") {{
    const next = value === "true" || value === "1" || value === "on" || value === element.value;
    element.checked = next;
    emit(element, "input");
    emit(element, "change");
    return true;
  }}
  if (typeof element.value === "string") {{
    element.value = value;
    emit(element, "input");
    emit(element, "change");
    return true;
  }}
  if (element.isContentEditable) {{
    element.textContent = value;
    emit(element, "input");
    emit(element, "change");
    return true;
  }}
  return false;
}})()"#
    )
    .into()
}

fn webview_submit_form_script(selector: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"form\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const form = String(target.tagName || "").toLowerCase() === "form"
    ? target
    : (target.form || (target.closest ? target.closest("form") : null));
  if (!form) return false;
  if (typeof form.requestSubmit === "function") {{
    form.requestSubmit();
    return true;
  }}
  let event;
  try {{
    event = new Event("submit", {{ bubbles: true, cancelable: true }});
  }} catch (_) {{
    event = document.createEvent("Event");
    event.initEvent("submit", true, true);
  }}
  const allowed = form.dispatchEvent(event);
  if (!allowed) return true;
  if (typeof form.submit === "function") {{
    form.submit();
    return true;
  }}
  return false;
}})()"#
    )
    .into()
}

fn webview_reset_form_script(selector: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"form\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const form = String(target.tagName || "").toLowerCase() === "form"
    ? target
    : (target.form || (target.closest ? target.closest("form") : null));
  if (!form || typeof form.reset !== "function") return false;
  form.reset();
  return true;
}})()"#
    )
    .into()
}

fn webview_selector_action_script(selector: &str, action: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    let action = serde_json::to_string(action).unwrap_or_else(|_| "\"focus\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const action = {action};
  let element = null;
  try {{
    element = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!element) return false;
  if (typeof element.scrollIntoView === "function") {{
    element.scrollIntoView({{ block: "nearest", inline: "nearest" }});
  }}
  if (action === "focus") {{
    if (typeof element.focus !== "function") return false;
    try {{
      element.focus({{ preventScroll: true }});
    }} catch (_) {{
      element.focus();
    }}
    return document.activeElement === element || element.contains(document.activeElement);
  }}
  if (action === "click") {{
    if (typeof element.click !== "function") return false;
    element.click();
    return true;
  }}
  return false;
}})()"#
    )
    .into()
}

fn webview_selected_text_script() -> SharedString {
    r#"(() => {
  const active = document.activeElement;
  if (active && typeof active.value === "string" && typeof active.selectionStart === "number" && typeof active.selectionEnd === "number") {
    return active.value.slice(active.selectionStart, active.selectionEnd);
  }
  const selection = window.getSelection && window.getSelection();
  return selection ? selection.toString() : "";
})()"#
        .into()
}

fn webview_selected_html_script() -> SharedString {
    r#"(() => {
  const container = document.createElement("div");
  const active = document.activeElement;
  if (active && typeof active.value === "string" && typeof active.selectionStart === "number" && typeof active.selectionEnd === "number") {
    container.textContent = active.value.slice(active.selectionStart, active.selectionEnd);
    return container.innerHTML;
  }
  const selection = window.getSelection && window.getSelection();
  if (!selection || selection.rangeCount === 0) return "";
  for (let index = 0; index < selection.rangeCount; index += 1) {
    container.appendChild(selection.getRangeAt(index).cloneContents());
  }
  return container.innerHTML;
})()"#
        .into()
}

/// Build a script that forwards browser selection snapshots through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, selectedText, selectedHtml, collapsed, editable, inputKind }`.
/// Inject this with [`WebViewOptions::selection_bridge`] or
/// [`WebView::selection_bridge`] when native edit menus, floating formatting
/// chrome, tests, or agents need to observe hosted selection state.
pub fn webview_selection_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"selection\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelSelectionBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  let scheduled = false;
  let pendingEvent = "initial";
  let lastKey = "";
  const activeInput = () => {{
    const active = document.activeElement;
    return active && typeof active.value === "string" &&
      typeof active.selectionStart === "number" &&
      typeof active.selectionEnd === "number"
      ? active
      : null;
  }};
  const inputKind = (element) => {{
    if (!element) return null;
    if (element.tagName === "TEXTAREA") return "textarea";
    if (element.tagName === "INPUT") return element.type || "input";
    if (element.isContentEditable) return "contenteditable";
    const editable = element.closest && element.closest("[contenteditable=''], [contenteditable='true']");
    return editable ? "contenteditable" : null;
  }};
  const selectedHtml = (selection) => {{
    const container = document.createElement("div");
    if (!selection || selection.rangeCount === 0) return "";
    for (let index = 0; index < selection.rangeCount; index += 1) {{
      container.appendChild(selection.getRangeAt(index).cloneContents());
    }}
    return container.innerHTML;
  }};
  const snapshot = (event) => {{
    const active = activeInput();
    if (active) {{
      const start = active.selectionStart || 0;
      const end = active.selectionEnd || start;
      const text = active.value.slice(start, end);
      const container = document.createElement("div");
      container.textContent = text;
      return {{
        event,
        selectedText: text,
        selectedHtml: container.innerHTML,
        collapsed: start === end,
        editable: true,
        inputKind: inputKind(active),
      }};
    }}
    const selection = window.getSelection && window.getSelection();
    const text = selection ? String(selection.toString() || "") : "";
    const anchor = selection && selection.anchorNode
      ? (selection.anchorNode.nodeType === Node.ELEMENT_NODE ? selection.anchorNode : selection.anchorNode.parentElement)
      : document.activeElement;
    const kind = inputKind(anchor);
    return {{
      event,
      selectedText: text,
      selectedHtml: selectedHtml(selection),
      collapsed: !selection || selection.isCollapsed || text.length === 0,
      editable: !!kind,
      inputKind: kind,
    }};
  }};
  const flush = () => {{
    scheduled = false;
    const current = snapshot(pendingEvent);
    const key = JSON.stringify(current);
    if (key === lastKey) return;
    lastKey = key;
    post(current);
  }};
  const schedule = (event) => {{
    pendingEvent = event;
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(flush);
  }};
  document.addEventListener("selectionchange", () => schedule("selectionchange"));
  document.addEventListener("select", () => schedule("select"), true);
  document.addEventListener("keyup", () => schedule("selectionchange"), true);
  document.addEventListener("mouseup", () => schedule("selectionchange"), true);
  document.addEventListener("touchend", () => schedule("selectionchange"), true);
  document.addEventListener("input", () => schedule("selectionchange"), true);
  window.addEventListener("focus", () => schedule("focus"), true);
  window.addEventListener("blur", () => schedule("blur"), true);
  schedule("initial");
}})();"#
    )
    .into()
}

fn webview_document_html_script() -> SharedString {
    r#"(() => {
  if (document.documentElement && typeof document.documentElement.outerHTML === "string") {
    return document.documentElement.outerHTML;
  }
  return "";
})()"#
        .into()
}

fn webview_document_snapshot_script() -> SharedString {
    r#"(() => {
  const nullable = (value) => value == null || value === "" ? null : String(value);
  const text = (value) => String(value == null ? "" : value).replace(/\s+/g, " ").trim();
  const attr = (element, name) => element && element.getAttribute ? nullable(element.getAttribute(name)) : null;
  const root = document.documentElement || null;
  const body = document.body || root;
  const visibleText = text(body ? (body.innerText || body.textContent || "") : "");
  const limit = 20000;
  const take = (items, map, count = 100) => Array.from(items || []).slice(0, count).map(map);
  const headings = take(document.querySelectorAll("h1,h2,h3,h4,h5,h6"), (heading) => ({
    level: Number((heading.tagName || "H0").slice(1)) || 0,
    text: text(heading.innerText || heading.textContent || ""),
    id: attr(heading, "id"),
  }));
  const links = take(document.querySelectorAll("a[href]"), (link) => ({
    text: text(link.innerText || link.textContent || ""),
    href: String(link.href || link.getAttribute("href") || ""),
    target: attr(link, "target"),
  }));
  const images = take(document.querySelectorAll("img[src]"), (image) => ({
    src: String(image.currentSrc || image.src || image.getAttribute("src") || ""),
    alt: String(image.alt || ""),
    title: attr(image, "title"),
  }));
  const forms = take(document.querySelectorAll("form"), (form) => ({
    id: attr(form, "id"),
    name: attr(form, "name"),
    action: String(form.action || form.getAttribute("action") || ""),
    method: String(form.method || "get").toLowerCase(),
    controlCount: form.elements ? form.elements.length : 0,
  }), 50);
  return {
    url: location.href || "",
    title: document.title || "",
    readyState: document.readyState || "",
    language: root ? nullable(root.lang || root.getAttribute("lang")) : null,
    direction: root ? nullable(root.dir || root.getAttribute("dir")) : null,
    visibleText: visibleText.slice(0, limit),
    textLength: visibleText.length,
    headings,
    links,
    images,
    forms,
  };
})()"#
    .into()
}

fn webview_element_snapshot_script(selector: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const nullable = (value) => value == null || value === "" ? null : String(value);
  const text = (value, limit = 4000) => String(value == null ? "" : value).replace(/\s+/g, " ").trim().slice(0, limit);
  let element = null;
  try {{
    element = document.querySelector(selector);
  }} catch (_) {{
    return null;
  }}
  if (!element || element.nodeType !== Node.ELEMENT_NODE) return null;
  const rect = element.getBoundingClientRect ? element.getBoundingClientRect() : {{ x: 0, y: 0, width: 0, height: 0 }};
  const computed = window.getComputedStyle ? window.getComputedStyle(element) : null;
  const attr = (name) => element.getAttribute ? nullable(element.getAttribute(name)) : null;
  const nearestLink = element.closest ? element.closest("a[href]") : null;
  const editable = !!(
    element.isContentEditable ||
    (element.closest && element.closest("[contenteditable=''], [contenteditable='true']"))
  );
  const attributes = Array.from(element.attributes || [])
    .slice(0, 100)
    .map((attribute) => ({{
      name: String(attribute.name || ""),
      value: String(attribute.value || "").slice(0, 4000),
    }}));
  const value = typeof element.value === "string" ? element.value.slice(0, 4000) : null;
  const checked = typeof element.checked === "boolean" ? !!element.checked : null;
  const src = element.currentSrc || element.src || attr("src");
  const hiddenByBox = Number(rect.width || 0) <= 0 || Number(rect.height || 0) <= 0;
  const display = computed ? String(computed.display || "") : "";
  const visibility = computed ? String(computed.visibility || "") : "";
  return {{
    url: location.href || "",
    selector,
    tagName: String(element.tagName || "").toLowerCase(),
    id: nullable(element.id),
    classes: Array.from(element.classList || []).map(String),
    text: text(element.innerText || element.textContent || ""),
    value,
    checked,
    disabled: !!element.disabled,
    hidden: !!element.hidden || display === "none" || visibility === "hidden" || hiddenByBox,
    editable,
    href: nearestLink ? String(nearestLink.href || nearestLink.getAttribute("href") || "") : null,
    src: src == null || src === "" ? null : String(src),
    rect: {{
      x: Number(rect.x || rect.left || 0),
      y: Number(rect.y || rect.top || 0),
      width: Number(rect.width || 0),
      height: Number(rect.height || 0),
    }},
    attributes,
    display,
    visibility,
    pointerEvents: computed ? String(computed.pointerEvents || "") : "",
  }};
}})()"#
    )
    .into()
}

fn webview_capture_dom_image_script(
    selector: &str,
    options: &WebViewDomImageCaptureOptions,
) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"body\"".into());
    let options = serde_json::to_string(options).unwrap_or_else(|_| "{}".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const options = {options};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return null;
  }}
  if (!target || !target.getBoundingClientRect) return null;
  const rect = target.getBoundingClientRect();
  const fallbackWidth = target.scrollWidth || target.clientWidth || document.documentElement.clientWidth || 1;
  const fallbackHeight = target.scrollHeight || target.clientHeight || document.documentElement.clientHeight || 1;
  let width = Number(options.width || 0);
  let height = Number(options.height || 0);
  if (!Number.isFinite(width) || width <= 0) width = Math.ceil(rect.width || fallbackWidth);
  if (!Number.isFinite(height) || height <= 0) height = Math.ceil(rect.height || fallbackHeight);
  width = Math.max(1, Math.round(width));
  height = Math.max(1, Math.round(height));
  const maxPixels = Number.isFinite(Number(options.maxPixels))
    ? Math.max(1, Number(options.maxPixels))
    : 4000000;
  const pixels = width * height;
  if (pixels > maxPixels) {{
    const scale = Math.sqrt(maxPixels / pixels);
    width = Math.max(1, Math.floor(width * scale));
    height = Math.max(1, Math.floor(height * scale));
  }}
  const clone = target.cloneNode(true);
  const copyState = (source, copy) => {{
    if (!(source instanceof Element) || !(copy instanceof Element)) return;
    if (source instanceof HTMLInputElement) {{
      copy.setAttribute("value", source.value || "");
      if (source.checked) copy.setAttribute("checked", "");
      else copy.removeAttribute("checked");
    }} else if (source instanceof HTMLTextAreaElement) {{
      copy.textContent = source.value || "";
    }} else if (source instanceof HTMLSelectElement) {{
      const sourceOptions = Array.from(source.options || []);
      const copyOptions = Array.from(copy.options || []);
      sourceOptions.forEach((option, index) => {{
        if (!copyOptions[index]) return;
        if (option.selected) copyOptions[index].setAttribute("selected", "");
        else copyOptions[index].removeAttribute("selected");
      }});
    }}
    const computed = window.getComputedStyle(source);
    let css = "";
    for (let index = 0; index < computed.length; index++) {{
      const name = computed[index];
      css += `${{name}}:${{computed.getPropertyValue(name)}};`;
    }}
    copy.setAttribute("style", css);
    const sourceChildren = Array.from(source.children || []);
    const copyChildren = Array.from(copy.children || []);
    sourceChildren.forEach((child, index) => copyState(child, copyChildren[index]));
  }};
  copyState(target, clone);
  clone.setAttribute("xmlns", "http://www.w3.org/1999/xhtml");
  clone.style.margin = "0";
  clone.style.boxSizing = "border-box";
  clone.style.width = `${{width}}px`;
  clone.style.minWidth = `${{width}}px`;
  clone.style.height = `${{height}}px`;
  clone.style.minHeight = `${{height}}px`;
  const background = typeof options.background === "string"
    ? options.background
    : "transparent";
  const html = new XMLSerializer().serializeToString(clone);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${{width}}" height="${{height}}" viewBox="0 0 ${{width}} ${{height}}"><foreignObject width="100%" height="100%"><div xmlns="http://www.w3.org/1999/xhtml" style="width:${{width}}px;height:${{height}}px;overflow:hidden;background:${{background}};">${{html}}</div></foreignObject></svg>`;
  return `data:image/svg+xml;charset=utf-8,${{encodeURIComponent(svg)}}`;
}})()"#
    )
    .into()
}

fn webview_trigger_download_script(url: &str, filename: Option<&SharedString>) -> SharedString {
    let url = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".into());
    let filename = serde_json::to_string(&filename.map(|value| value.as_ref()))
        .unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const requestedUrl = {url};
  const requestedFilename = {filename};
  try {{
    const resolvedUrl = new URL(String(requestedUrl || ""), document.baseURI || location.href).href;
    const anchor = document.createElement("a");
    anchor.href = resolvedUrl;
    anchor.rel = "noopener";
    anchor.style.display = "none";
    if (requestedFilename != null && String(requestedFilename).length > 0) {{
      anchor.download = String(requestedFilename);
    }} else {{
      anchor.download = "";
    }}
    document.documentElement.appendChild(anchor);
    anchor.click();
    anchor.remove();
    return {{
      ok: true,
      url: resolvedUrl,
      filename: requestedFilename == null || String(requestedFilename).length === 0 ? null : String(requestedFilename),
      error: null,
    }};
  }} catch (error) {{
    return {{
      ok: false,
      url: String(requestedUrl || ""),
      filename: requestedFilename == null || String(requestedFilename).length === 0 ? null : String(requestedFilename),
      error: error && error.message ? String(error.message) : String(error),
    }};
  }}
}})()"#
    )
    .into()
}

fn webview_storage_snapshot_script() -> SharedString {
    r#"(() => {
  const readArea = (area, storage) => {
    try {
      if (!storage) {
        return { area, available: false, length: 0, entries: [], error: "storage area is unavailable" };
      }
      const length = Number(storage.length || 0);
      const entries = [];
      for (let index = 0; index < length; index += 1) {
        const key = storage.key(index);
        if (key == null) continue;
        entries.push({ key: String(key), value: String(storage.getItem(key) ?? "") });
      }
      return { area, available: true, length, entries, error: null };
    } catch (error) {
      return {
        area,
        available: false,
        length: 0,
        entries: [],
        error: error && error.message ? String(error.message) : String(error),
      };
    }
  };
  return {
    url: location.href || "",
    origin: location.origin || "",
    localStorage: readArea("localStorage", window.localStorage),
    sessionStorage: readArea("sessionStorage", window.sessionStorage),
  };
})()"#
        .into()
}

fn webview_storage_mutation_result_script(
    action: &str,
    area: WebViewStorageArea,
    key: Option<&str>,
    value: Option<&str>,
) -> SharedString {
    let action = serde_json::to_string(action).unwrap_or_else(|_| "\"setItem\"".into());
    let area = serde_json::to_string(area.as_str()).unwrap_or_else(|_| "\"localStorage\"".into());
    let key = serde_json::to_string(&key).unwrap_or_else(|_| "null".into());
    let value = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const action = {action};
  const area = {area};
  const key = {key};
  const value = {value};
  const storage = area === "sessionStorage" ? window.sessionStorage : window.localStorage;
  const length = () => {{
    try {{ return storage && Number.isFinite(storage.length) ? storage.length : 0; }}
    catch (_) {{ return 0; }}
  }};
  try {{
    if (!storage) return {{ ok: false, area, key, length: 0, error: "storage area is unavailable" }};
    if (action === "setItem") {{
      storage.setItem(String(key), String(value));
      return {{ ok: true, area, key: String(key), length: length(), error: null }};
    }}
    if (action === "removeItem") {{
      storage.removeItem(String(key));
      return {{ ok: true, area, key: String(key), length: length(), error: null }};
    }}
    if (action === "clear") {{
      storage.clear();
      return {{ ok: true, area, key: null, length: length(), error: null }};
    }}
    return {{ ok: false, area, key, length: length(), error: `unsupported storage action: ${{action}}` }};
  }} catch (error) {{
    return {{
      ok: false,
      area,
      key,
      length: length(),
      error: error && error.message ? String(error.message) : String(error),
    }};
  }}
}})()"#
    )
    .into()
}

fn webview_set_storage_item_script(
    area: WebViewStorageArea,
    key: &str,
    value: &str,
) -> SharedString {
    webview_storage_mutation_result_script("setItem", area, Some(key), Some(value))
}

fn webview_remove_storage_item_script(area: WebViewStorageArea, key: &str) -> SharedString {
    webview_storage_mutation_result_script("removeItem", area, Some(key), None)
}

fn webview_clear_storage_area_script(area: WebViewStorageArea) -> SharedString {
    webview_storage_mutation_result_script("clear", area, None, None)
}

fn webview_scroll_number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "0".into()
    }
}

fn webview_viewport_snapshot_body(event: &str) -> String {
    let event = serde_json::to_string(event).unwrap_or_else(|_| "\"snapshot\"".into());
    format!(
        r#"  const number = (value) => Number.isFinite(value) ? value : 0;
  const snapshot = (event) => {{
    const root = document.scrollingElement || document.documentElement || document.body;
    const body = document.body || root;
    const viewportWidth = number(window.innerWidth || document.documentElement.clientWidth || 0);
    const viewportHeight = number(window.innerHeight || document.documentElement.clientHeight || 0);
    const scrollWidth = number(Math.max(root ? root.scrollWidth : 0, body ? body.scrollWidth : 0, viewportWidth));
    const scrollHeight = number(Math.max(root ? root.scrollHeight : 0, body ? body.scrollHeight : 0, viewportHeight));
    const maxX = Math.max(0, scrollWidth - viewportWidth);
    const maxY = Math.max(0, scrollHeight - viewportHeight);
    const x = Math.min(Math.max(0, number(window.scrollX || (root && root.scrollLeft) || 0)), maxX);
    const y = Math.min(Math.max(0, number(window.scrollY || (root && root.scrollTop) || 0)), maxY);
    return {{
      event,
      x,
      y,
      maxX,
      maxY,
      viewportWidth,
      viewportHeight,
      scrollWidth,
      scrollHeight,
      progressX: maxX > 0 ? x / maxX : 0,
      progressY: maxY > 0 ? y / maxY : 0,
    }};
  }};
  return snapshot({event});"#
    )
}

fn webview_viewport_snapshot_script(event: &str) -> SharedString {
    format!(
        r#"(() => {{
{}
}})()"#,
        webview_viewport_snapshot_body(event)
    )
    .into()
}

fn webview_scroll_to_script(x: f64, y: f64) -> SharedString {
    let x = webview_scroll_number(x);
    let y = webview_scroll_number(y);
    format!(
        r#"(() => {{
  const targetX = {x};
  const targetY = {y};
  window.scrollTo({{ left: targetX, top: targetY, behavior: "auto" }});
{}
}})()"#,
        webview_viewport_snapshot_body("scroll")
    )
    .into()
}

fn webview_scroll_by_script(delta_x: f64, delta_y: f64) -> SharedString {
    let delta_x = webview_scroll_number(delta_x);
    let delta_y = webview_scroll_number(delta_y);
    format!(
        r#"(() => {{
  const deltaX = {delta_x};
  const deltaY = {delta_y};
  window.scrollBy({{ left: deltaX, top: deltaY, behavior: "auto" }});
{}
}})()"#,
        webview_viewport_snapshot_body("scroll")
    )
    .into()
}

fn webview_scroll_selector_into_view_script(selector: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return null;
  }}
  if (!target || typeof target.scrollIntoView !== "function") return null;
  target.scrollIntoView({{ block: "nearest", inline: "nearest", behavior: "auto" }});
{}
}})()"#,
        webview_viewport_snapshot_body("scrollIntoView")
    )
    .into()
}

fn webview_favicons_script() -> SharedString {
    r#"(() => {
  const selectors = [
    "link[rel~='icon'][href]",
    "link[rel='shortcut icon'][href]",
    "link[rel~='apple-touch-icon'][href]",
    "link[rel~='mask-icon'][href]",
  ];
  const urls = [];
  const seen = new Set();
  for (const link of Array.from(document.querySelectorAll(selectors.join(",")))) {
    const href = link.href || link.getAttribute("href") || "";
    if (href && !seen.has(href)) {
      seen.add(href);
      urls.push(href);
    }
  }
  return urls;
})()"#
        .into()
}

fn webview_insert_css_script(key: &str, css: &str) -> SharedString {
    let key = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".into());
    let css = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(() => {{ const key = {key}; const css = {css}; const target = document.head || document.documentElement; if (!target) return; let style = Array.from(document.querySelectorAll('style[data-kael-style-key]')).find((node) => node.getAttribute('data-kael-style-key') === key); if (!style) {{ style = document.createElement('style'); style.type = 'text/css'; style.setAttribute('data-kael-style-key', key); target.appendChild(style); }} style.textContent = css; }})();"
    )
    .into()
}

fn webview_remove_inserted_css_script(key: &str) -> SharedString {
    let key = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(() => {{ const key = {key}; const style = Array.from(document.querySelectorAll('style[data-kael-style-key]')).find((node) => node.getAttribute('data-kael-style-key') === key); if (style && style.parentNode) style.parentNode.removeChild(style); }})();"
    )
    .into()
}

fn webview_pause_media_script() -> SharedString {
    "(() => { for (const element of Array.from(document.querySelectorAll('audio,video'))) { if (typeof element.pause === 'function') element.pause(); } })();".into()
}

fn webview_play_media_script() -> SharedString {
    "(() => { for (const element of Array.from(document.querySelectorAll('audio,video'))) { if (typeof element.play === 'function') { const result = element.play(); if (result && typeof result.catch === 'function') result.catch(() => {}); } } })();".into()
}

fn webview_set_media_muted_script(muted: bool) -> SharedString {
    format!(
        "(() => {{ for (const element of Array.from(document.querySelectorAll('audio,video'))) {{ element.muted = {}; }} }})();",
        if muted { "true" } else { "false" },
    )
    .into()
}

fn webview_set_media_volume_script(volume: f32) -> SharedString {
    let volume = if volume.is_finite() { volume } else { 1.0 };
    let volume = volume.clamp(0.0, 1.0);
    format!(
        "(() => {{ const value = {}; for (const element of Array.from(document.querySelectorAll('audio,video'))) {{ element.volume = value; }} }})();",
        volume
    )
    .into()
}

fn webview_set_media_playback_rate_script(rate: f32) -> SharedString {
    let rate = if rate.is_finite() { rate.max(0.0) } else { 1.0 };
    format!(
        "(() => {{ const value = {}; for (const element of Array.from(document.querySelectorAll('audio,video'))) {{ element.playbackRate = value; }} }})();",
        rate
    )
    .into()
}

fn webview_seek_media_secs_script(seconds: f64) -> SharedString {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    format!(
        "(() => {{ const value = {}; for (const element of Array.from(document.querySelectorAll('audio,video'))) {{ try {{ element.currentTime = value; }} catch (_) {{}} }} }})();",
        seconds
    )
    .into()
}

fn webview_media_command_script(selector: &str, command: WebViewMediaCommand) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"audio,video\"".into());
    let command = command.json();
    format!(
        r#"(() => {{
  const selector = {selector};
  const command = {command};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const tag = String(target.tagName || "").toLowerCase();
  const media = tag === "audio" || tag === "video"
    ? target
    : (target.closest ? target.closest("audio,video") : null);
  if (!media) return false;
  const safePlay = () => {{
    if (typeof media.play !== "function") return;
    const result = media.play();
    if (result && typeof result.catch === "function") result.catch(() => {{}});
  }};
  switch (command.action) {{
    case "play":
      safePlay();
      break;
    case "pause":
      if (typeof media.pause === "function") media.pause();
      break;
    case "togglePlay":
      if (media.paused) safePlay();
      else if (typeof media.pause === "function") media.pause();
      break;
    case "stop":
      if (typeof media.pause === "function") media.pause();
      try {{ media.currentTime = 0; }} catch (_) {{}}
      break;
    case "setMuted":
      media.muted = !!command.value;
      break;
    case "setVolume":
      media.volume = Math.min(1, Math.max(0, Number(command.value) || 0));
      break;
    case "setPlaybackRate":
      media.playbackRate = Math.max(0, Number(command.value) || 0);
      break;
    case "seek":
      try {{ media.currentTime = Math.max(0, Number(command.value) || 0); }} catch (_) {{}}
      break;
    default:
      return false;
  }}
  return true;
}})()"#
    )
    .into()
}

fn webview_set_media_source_script(selector: &str, source: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"audio,video\"".into());
    let source = serde_json::to_string(source).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const source = {source};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const tag = String(target.tagName || "").toLowerCase();
  const media = tag === "audio" || tag === "video"
    ? target
    : (tag === "source" && target.parentElement &&
       /^(audio|video)$/i.test(target.parentElement.tagName)
        ? target.parentElement
        : (target.closest ? target.closest("audio,video") : null));
  if (!media) return false;
  if (tag === "source") target.src = source;
  media.src = source;
  if (typeof media.load === "function") media.load();
  return true;
}})()"#
    )
    .into()
}

fn webview_set_media_options_script(
    selector: &str,
    options: &WebViewMediaElementOptions,
) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"audio,video\"".into());
    let options = serde_json::to_string(options).unwrap_or_else(|_| "{}".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const options = {options};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const tag = String(target.tagName || "").toLowerCase();
  const media = tag === "audio" || tag === "video"
    ? target
    : (target.closest ? target.closest("audio,video") : null);
  if (!media) return false;
  const mediaTag = String(media.tagName || "").toLowerCase();
  const has = (name) => Object.prototype.hasOwnProperty.call(options, name);
  const boolProp = (prop, attr) => {{
    if (!has(prop)) return;
    const value = !!options[prop];
    media[prop] = value;
    if (value) media.setAttribute(attr, "");
    else media.removeAttribute(attr);
  }};
  boolProp("controls", "controls");
  boolProp("loop", "loop");
  boolProp("autoplay", "autoplay");
  boolProp("muted", "muted");
  boolProp("playsInline", "playsinline");
  boolProp("disablePictureInPicture", "disablepictureinpicture");
  if (has("poster") && typeof options.poster === "string" && mediaTag === "video") {{
    media.poster = options.poster;
  }}
  if (has("preload") && typeof options.preload === "string") {{
    media.preload = options.preload;
  }}
  if (Array.isArray(options.controlsList)) {{
    const tokens = options.controlsList.map((item) => String(item || "").trim()).filter(Boolean);
    if (media.controlsList && typeof media.controlsList.value === "string") {{
      media.controlsList.value = tokens.join(" ");
    }} else if (tokens.length) {{
      media.setAttribute("controlslist", tokens.join(" "));
    }} else {{
      media.removeAttribute("controlslist");
    }}
  }}
  return true;
}})()"#
    )
    .into()
}

fn webview_capture_media_frame_script(
    selector: &str,
    options: &WebViewMediaFrameCaptureOptions,
) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"video\"".into());
    let options = serde_json::to_string(options).unwrap_or_else(|_| "{}".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const options = {options};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return null;
  }}
  if (!target) return null;
  const tag = String(target.tagName || "").toLowerCase();
  const video = tag === "video" ? target : (target.closest ? target.closest("video") : null);
  if (!video || !video.videoWidth || !video.videoHeight) return null;
  const sourceWidth = video.videoWidth;
  const sourceHeight = video.videoHeight;
  let width = Number(options.width || 0);
  let height = Number(options.height || 0);
  if (!Number.isFinite(width) || width <= 0) width = 0;
  if (!Number.isFinite(height) || height <= 0) height = 0;
  if (!width && !height) {{
    width = sourceWidth;
    height = sourceHeight;
  }} else if (width && !height) {{
    height = Math.max(1, Math.round(width * sourceHeight / sourceWidth));
  }} else if (!width && height) {{
    width = Math.max(1, Math.round(height * sourceWidth / sourceHeight));
  }}
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width));
  canvas.height = Math.max(1, Math.round(height));
  const context = canvas.getContext("2d");
  if (!context) return null;
  try {{
    context.drawImage(video, 0, 0, canvas.width, canvas.height);
    const mimeType = typeof options.mimeType === "string" && options.mimeType
      ? options.mimeType
      : "image/png";
    const quality = Number.isFinite(Number(options.quality))
      ? Math.min(1, Math.max(0, Number(options.quality)))
      : undefined;
    return quality == null ? canvas.toDataURL(mimeType) : canvas.toDataURL(mimeType, quality);
  }} catch (_) {{
    return null;
  }}
}})()"#
    )
    .into()
}

fn webview_add_media_text_track_script(
    selector: &str,
    track: &WebViewMediaTextTrackOptions,
) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"audio,video\"".into());
    let track = serde_json::to_string(track).unwrap_or_else(|_| "{}".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const trackOptions = {track};
  if (!trackOptions || typeof trackOptions.src !== "string" || !trackOptions.src) return false;
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const tag = String(target.tagName || "").toLowerCase();
  const media = tag === "audio" || tag === "video"
    ? target
    : (target.closest ? target.closest("audio,video") : null);
  if (!media) return false;
  const track = document.createElement("track");
  track.src = trackOptions.src;
  track.kind = typeof trackOptions.kind === "string" && trackOptions.kind ? trackOptions.kind : "subtitles";
  if (typeof trackOptions.label === "string") track.label = trackOptions.label;
  if (typeof trackOptions.language === "string") track.srclang = trackOptions.language;
  if (typeof trackOptions.id === "string") track.id = trackOptions.id;
  if (trackOptions.default === true) track.default = true;
  media.appendChild(track);
  if (typeof trackOptions.mode === "string") {{
    const applyMode = () => {{
      if (track.track) track.track.mode = trackOptions.mode;
    }};
    applyMode();
    track.addEventListener("load", applyMode, {{ once: true }});
  }}
  return true;
}})()"#
    )
    .into()
}

fn webview_remove_media_text_track_script(selector: &str, track_selector: &str) -> SharedString {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"audio,video\"".into());
    let track_selector = serde_json::to_string(track_selector).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const selector = {selector};
  const trackSelector = {track_selector};
  let target = null;
  try {{
    target = document.querySelector(selector);
  }} catch (_) {{
    return false;
  }}
  if (!target) return false;
  const tag = String(target.tagName || "").toLowerCase();
  const media = tag === "audio" || tag === "video"
    ? target
    : (target.closest ? target.closest("audio,video") : null);
  if (!media) return false;
  const tracks = Array.from(media.querySelectorAll("track"));
  let removed = false;
  for (const [index, element] of tracks.entries()) {{
    const browserTrack = element.track || {{}};
    const matches = String(index) === trackSelector ||
      element.id === trackSelector ||
      element.label === trackSelector ||
      element.srclang === trackSelector ||
      element.kind === trackSelector ||
      element.src === trackSelector ||
      browserTrack.id === trackSelector ||
      browserTrack.label === trackSelector ||
      browserTrack.language === trackSelector ||
      browserTrack.kind === trackSelector;
    if (matches && element.parentNode) {{
      if (browserTrack) browserTrack.mode = "disabled";
      element.parentNode.removeChild(element);
      removed = true;
    }}
  }}
  return removed;
}})()"#
    )
    .into()
}

fn webview_select_media_text_track_script(selector: impl AsRef<str>) -> SharedString {
    let selector = serde_json::to_string(selector.as_ref()).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(() => {{
  const selector = {selector};
  for (const element of Array.from(document.querySelectorAll('audio,video'))) {{
    for (const [index, track] of Array.from(element.textTracks || []).entries()) {{
      const matches = String(index) === selector || track.id === selector || track.label === selector || track.language === selector;
      track.mode = matches ? 'showing' : 'disabled';
    }}
  }}
}})();"#
    )
    .into()
}

fn webview_disable_media_text_tracks_script() -> SharedString {
    r#"(() => {
  for (const element of Array.from(document.querySelectorAll('audio,video'))) {
    for (const track of Array.from(element.textTracks || [])) {
      track.mode = 'disabled';
    }
  }
})();"#
        .into()
}

fn webview_request_media_fullscreen_script() -> SharedString {
    r#"(() => {
  const element = document.querySelector('video') || document.querySelector('audio,video');
  if (!element) return;
  const request = element.requestFullscreen || element.webkitRequestFullscreen || element.msRequestFullscreen;
  if (typeof request !== 'function') return;
  const result = request.call(element);
  if (result && typeof result.catch === 'function') result.catch(() => {});
})();"#
        .into()
}

fn webview_exit_media_fullscreen_script() -> SharedString {
    r#"(() => {
  const exit = document.exitFullscreen || document.webkitExitFullscreen || document.msExitFullscreen;
  if (typeof exit !== 'function') return;
  const result = exit.call(document);
  if (result && typeof result.catch === 'function') result.catch(() => {});
})();"#
        .into()
}

fn webview_request_media_picture_in_picture_script() -> SharedString {
    r#"(() => {
  const video = document.querySelector('video');
  if (!video || typeof video.requestPictureInPicture !== 'function') return;
  const result = video.requestPictureInPicture();
  if (result && typeof result.catch === 'function') result.catch(() => {});
})();"#
        .into()
}

fn webview_exit_media_picture_in_picture_script() -> SharedString {
    r#"(() => {
  if (!document.pictureInPictureElement || typeof document.exitPictureInPicture !== 'function') return;
  const result = document.exitPictureInPicture();
  if (result && typeof result.catch === 'function') result.catch(() => {});
})();"#
        .into()
}

fn webview_media_state_script() -> SharedString {
    r#"(() => Array.from(document.querySelectorAll('audio,video')).map((element, index) => {
  const finiteOrNull = (value) => Number.isFinite(value) ? value : null;
  const finiteOrZero = (value) => Number.isFinite(value) ? value : 0;
  const buffered = [];
  if (element.buffered) {
    for (let i = 0; i < element.buffered.length; i += 1) {
      buffered.push({ start: element.buffered.start(i), end: element.buffered.end(i) });
    }
  }
  const textTracks = Array.from(element.textTracks || []).map((track, trackIndex) => ({
    index: trackIndex,
    id: track.id || null,
    kind: track.kind || "",
    label: track.label || "",
    language: track.language || "",
    mode: track.mode || "",
    activeCues: Array.from(track.activeCues || []).map((cue) => ({
      id: cue.id || null,
      startTime: finiteOrZero(cue.startTime),
      endTime: finiteOrZero(cue.endTime),
      text: cue.text || "",
    })),
  }));
  return {
    index,
    tagName: String(element.tagName || '').toLowerCase(),
    id: element.id || null,
    src: element.currentSrc || element.src || null,
    paused: !!element.paused,
    ended: !!element.ended,
    muted: !!element.muted,
    volume: finiteOrZero(element.volume),
    playbackRate: finiteOrZero(element.playbackRate),
    currentTime: finiteOrZero(element.currentTime),
    duration: finiteOrNull(element.duration),
    readyState: element.readyState || 0,
    networkState: element.networkState || 0,
    seeking: !!element.seeking,
    fullscreen: document.fullscreenElement === element || document.webkitFullscreenElement === element || document.msFullscreenElement === element,
    pictureInPicture: document.pictureInPictureElement === element,
    buffered,
    textTracks,
  };
}))()"#
        .into()
}

/// Build a script that forwards browser media element events through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, state }`, where `state` matches [`WebViewMediaElementState`].
/// Inject this with [`WebViewOptions::media_event_bridge`] or
/// [`WebView::media_event_bridge`] when native chrome should react to
/// WebView-hosted `<audio>` / `<video>` changes without polling.
pub fn webview_media_event_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"media-event\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelMediaEventBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const events = ["play", "playing", "pause", "ended", "timeupdate", "seeking", "seeked", "volumechange", "ratechange", "loadedmetadata", "durationchange", "progress", "waiting", "canplay", "canplaythrough", "error"];
  const finiteOrNull = (value) => Number.isFinite(value) ? value : null;
  const finiteOrZero = (value) => Number.isFinite(value) ? value : 0;
  const mediaElements = () => Array.from(document.querySelectorAll("audio,video"));
  const stateFor = (element) => {{
    const buffered = [];
    if (element.buffered) {{
      for (let i = 0; i < element.buffered.length; i += 1) {{
        buffered.push({{ start: element.buffered.start(i), end: element.buffered.end(i) }});
      }}
    }}
    const textTracks = Array.from(element.textTracks || []).map((track, trackIndex) => ({{
      index: trackIndex,
      id: track.id || null,
      kind: track.kind || "",
      label: track.label || "",
      language: track.language || "",
      mode: track.mode || "",
      activeCues: Array.from(track.activeCues || []).map((cue) => ({{
        id: cue.id || null,
        startTime: finiteOrZero(cue.startTime),
        endTime: finiteOrZero(cue.endTime),
        text: cue.text || "",
      }})),
    }}));
    return {{
      index: mediaElements().indexOf(element),
      tagName: String(element.tagName || "").toLowerCase(),
      id: element.id || null,
      src: element.currentSrc || element.src || null,
      paused: !!element.paused,
      ended: !!element.ended,
      muted: !!element.muted,
      volume: finiteOrZero(element.volume),
      playbackRate: finiteOrZero(element.playbackRate),
      currentTime: finiteOrZero(element.currentTime),
      duration: finiteOrNull(element.duration),
      readyState: element.readyState || 0,
      networkState: element.networkState || 0,
      seeking: !!element.seeking,
      fullscreen: document.fullscreenElement === element || document.webkitFullscreenElement === element || document.msFullscreenElement === element,
      pictureInPicture: document.pictureInPictureElement === element,
      buffered,
      textTracks,
    }};
  }};
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const bind = (element) => {{
    if (element.__kaelMediaEventBridgeBound) return;
    element.__kaelMediaEventBridgeBound = true;
    for (const event of events) {{
      element.addEventListener(event, () => post({{ event, state: stateFor(element) }}));
    }}
  }};
  const bindAll = () => mediaElements().forEach(bind);
  bindAll();
  new MutationObserver(bindAll).observe(document.documentElement || document, {{ childList: true, subtree: true }});
}})();"#
    )
    .into()
}

/// Build a script that forwards browser `contextmenu` events through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ x, y, selectedText, linkHref, imageSrc, mediaSrc, editable, inputKind }`.
/// Inject this with [`WebViewOptions::context_menu_bridge`] or
/// [`WebView::context_menu_bridge`] when native chrome should own right-click
/// menus for WebView content. The browser default context menu is prevented so
/// the app can render a native menu instead.
pub fn webview_context_menu_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"context-menu\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelContextMenuBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const selectedText = () => {{
    const active = document.activeElement;
    if (active && (active.tagName === "TEXTAREA" || (active.tagName === "INPUT" && typeof active.selectionStart === "number"))) {{
      const start = active.selectionStart || 0;
      const end = active.selectionEnd || start;
      return String(active.value || "").slice(start, end);
    }}
    const selection = window.getSelection && window.getSelection();
    return selection ? String(selection.toString() || "") : "";
  }};
  const closest = (element, selector) => element && element.closest ? element.closest(selector) : null;
  document.addEventListener("contextmenu", (event) => {{
    const rawTarget = event.target;
    const element = rawTarget && rawTarget.nodeType === Node.ELEMENT_NODE ? rawTarget : rawTarget && rawTarget.parentElement;
    const link = closest(element, "a[href]");
    const image = closest(element, "img");
    const media = closest(element, "audio,video");
    const editableElement = closest(element, "input, textarea, [contenteditable=''], [contenteditable='true']");
    const input = closest(element, "input, textarea");
    const payload = {{
      x: Number.isFinite(event.clientX) ? event.clientX : 0,
      y: Number.isFinite(event.clientY) ? event.clientY : 0,
      selectedText: selectedText(),
      linkHref: link ? link.href : null,
      imageSrc: image ? (image.currentSrc || image.src || null) : null,
      mediaSrc: media ? (media.currentSrc || media.src || null) : null,
      editable: !!(editableElement || (element && element.isContentEditable)),
      inputKind: input ? (input.type || String(input.tagName || "").toLowerCase()) : null,
    }};
    event.preventDefault();
    post(payload);
  }}, true);
}})();"#
    )
    .into()
}

/// Build a script that forwards browser pointer context through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, x, y, buttons, pointerType, targetTag, linkHref, imageSrc,
/// mediaSrc, editable, inputKind }`. Inject this with
/// [`WebViewOptions::pointer_bridge`] or [`WebView::pointer_bridge`] when native
/// status bars, hover previews, tests, or agents need lightweight page context
/// for hovered and clicked content.
pub fn webview_pointer_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"pointer\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelPointerBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const closest = (element, selector) => element && element.closest ? element.closest(selector) : null;
  const payloadFor = (event) => {{
    const rawTarget = event.target;
    const element = rawTarget && rawTarget.nodeType === Node.ELEMENT_NODE ? rawTarget : rawTarget && rawTarget.parentElement;
    const link = closest(element, "a[href]");
    const image = closest(element, "img");
    const media = closest(element, "audio,video");
    const editableElement = closest(element, "input, textarea, [contenteditable=''], [contenteditable='true']");
    const input = closest(element, "input, textarea");
    return {{
      event: event.type || "",
      x: Number.isFinite(event.clientX) ? event.clientX : 0,
      y: Number.isFinite(event.clientY) ? event.clientY : 0,
      buttons: Number.isFinite(event.buttons) ? event.buttons : 0,
      pointerType: event.pointerType || (event.type && event.type.startsWith("mouse") ? "mouse" : ""),
      targetTag: element ? String(element.tagName || "").toUpperCase() : null,
      linkHref: link ? link.href : null,
      imageSrc: image ? (image.currentSrc || image.src || null) : null,
      mediaSrc: media ? (media.currentSrc || media.src || null) : null,
      editable: !!(editableElement || (element && element.isContentEditable)),
      inputKind: input ? (input.type || String(input.tagName || "").toLowerCase()) : null,
    }};
  }};
  let scheduled = false;
  let pendingEvent = null;
  let lastMoveKey = "";
  const flushMove = () => {{
    scheduled = false;
    if (!pendingEvent) return;
    const payload = payloadFor(pendingEvent);
    const key = JSON.stringify(payload);
    if (key !== lastMoveKey) {{
      lastMoveKey = key;
      post(payload);
    }}
    pendingEvent = null;
  }};
  const scheduleMove = (event) => {{
    pendingEvent = event;
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(flushMove);
  }};
  const emit = (event) => post(payloadFor(event));
  document.addEventListener("pointermove", scheduleMove, {{ passive: true, capture: true }});
  document.addEventListener("pointerdown", emit, true);
  document.addEventListener("pointerup", emit, true);
  document.addEventListener("click", emit, true);
  document.addEventListener("dblclick", emit, true);
  document.addEventListener("pointerleave", emit, true);
}})();"#
    )
    .into()
}

/// Build a script that forwards browser form activity through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, formId, formName, action, method, target, enctype, field, fields,
/// defaultPrevented }`. Inject this with [`WebViewOptions::form_bridge`] or
/// [`WebView::form_bridge`] when native chrome, validation, tests, or agents
/// need to observe hosted form submit/reset/change/input activity without
/// writing custom page JavaScript. Password and file input values are omitted.
pub fn webview_form_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"form\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelFormBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const nullable = (value) => value === undefined || value === null || value === "" ? null : String(value);
  const tagName = (element) => String((element && element.tagName) || "").toLowerCase();
  const inputKind = (element) => {{
    const tag = tagName(element);
    if (tag === "input") return String(element.type || "text").toLowerCase();
    if (tag === "select") return element.multiple ? "select-multiple" : "select-one";
    return tag;
  }};
  const isSensitive = (element) => {{
    const tag = tagName(element);
    const type = String((element && element.type) || "").toLowerCase();
    return tag === "input" && (type === "password" || type === "file");
  }};
  const controlState = (element) => {{
    if (!element || !("name" in element || "value" in element || "checked" in element)) return null;
    const tag = tagName(element);
    if (!tag || tag === "fieldset" || tag === "output" || tag === "button") return null;
    const kind = inputKind(element);
    let value = null;
    if (!isSensitive(element)) {{
      if (tag === "select" && element.multiple) {{
        value = Array.from(element.selectedOptions || []).map((option) => String(option.value || ""));
      }} else if ("value" in element) {{
        value = String(element.value || "");
      }}
    }}
    return {{
      name: nullable(element.name),
      id: nullable(element.id),
      tagName: tag,
      inputKind: kind,
      value,
      checked: "checked" in element ? !!element.checked : null,
      disabled: !!element.disabled,
      required: !!element.required,
    }};
  }};
  const controls = (form) => Array.from((form && form.elements) || [])
    .map(controlState)
    .filter(Boolean);
  const closestForm = (element) => {{
    if (!element) return null;
    if (element.form) return element.form;
    return element.closest ? element.closest("form") : null;
  }};
  const formMeta = (form) => ({{
    formId: form ? nullable(form.id) : null,
    formName: form ? nullable(form.name) : null,
    action: form ? nullable(form.action || form.getAttribute("action")) : null,
    method: form ? String(form.method || "get").toLowerCase() : "",
    target: form ? nullable(form.target) : null,
    enctype: form ? nullable(form.enctype || form.encoding) : null,
  }});
  const payloadFor = (event, includeFields) => {{
    const target = event.target && event.target.nodeType === Node.ELEMENT_NODE
      ? event.target
      : event.target && event.target.parentElement;
    const submitter = event.submitter && event.submitter.nodeType === Node.ELEMENT_NODE
      ? event.submitter
      : null;
    const form = tagName(target) === "form" ? target : closestForm(target);
    return {{
      event: event.type || "",
      ...formMeta(form),
      field: controlState(submitter || (tagName(target) === "form" ? null : target)),
      fields: includeFields ? controls(form) : [],
      defaultPrevented: !!event.defaultPrevented,
    }};
  }};
  document.addEventListener("submit", (event) => post(payloadFor(event, true)), true);
  document.addEventListener("reset", (event) => post(payloadFor(event, true)), true);
  document.addEventListener("change", (event) => post(payloadFor(event, false)), true);
  document.addEventListener("input", (event) => post(payloadFor(event, false)), true);
}})();"#
    )
    .into()
}

/// Build a script that forwards browser file-input selections through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, inputName, inputId, accept, multiple, formId, formName, action,
/// method, files }`, where each file includes browser-exposed name, size,
/// MIME type, and last modified timestamp. Browsers intentionally do not expose
/// selected file paths.
pub fn webview_file_input_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"file-input\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelFileInputBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const nullable = (value) => value === undefined || value === null || value === "" ? null : String(value);
  const isFileInput = (element) => element &&
    String(element.tagName || "").toLowerCase() === "input" &&
    String(element.type || "").toLowerCase() === "file";
  const fileState = (file) => ({{
    name: String(file.name || ""),
    size: Number.isFinite(file.size) ? file.size : 0,
    mimeType: nullable(file.type),
    lastModified: Number.isFinite(file.lastModified) ? file.lastModified : null,
  }});
  const formMeta = (form) => ({{
    formId: form ? nullable(form.id) : null,
    formName: form ? nullable(form.name) : null,
    action: form ? nullable(form.action || form.getAttribute("action")) : null,
    method: form ? String(form.method || "get").toLowerCase() : "",
  }});
  const payloadFor = (event) => {{
    const input = event.target && event.target.nodeType === Node.ELEMENT_NODE
      ? event.target
      : event.target && event.target.parentElement;
    if (!isFileInput(input)) return null;
    const form = input.form || (input.closest ? input.closest("form") : null);
    return {{
      event: event.type || "",
      inputName: nullable(input.name),
      inputId: nullable(input.id),
      accept: nullable(input.accept),
      multiple: !!input.multiple,
      ...formMeta(form),
      files: Array.from(input.files || []).map(fileState),
    }};
  }};
  const emit = (event) => {{
    const payload = payloadFor(event);
    if (payload) post(payload);
  }};
  document.addEventListener("change", emit, true);
  document.addEventListener("input", emit, true);
}})();"#
    )
    .into()
}

/// Build a script that forwards browser resource timing and load/error activity.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, url, initiatorType, targetTag, success, startTime, duration,
/// transferSize, encodedBodySize, decodedBodySize, nextHopProtocol,
/// renderBlockingStatus }`. This is an observability bridge for native
/// diagnostics, test harnesses, and agents; it does not intercept or rewrite
/// requests.
pub fn webview_resource_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"resource\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelResourceBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const nullable = (value) => value === undefined || value === null || value === "" ? null : String(value);
  const finite = (value) => Number.isFinite(value) ? value : 0;
  const seen = new Set();
  const emit = (payload) => {{
    if (!payload || !payload.url) return;
    const key = [
      payload.event || "",
      payload.url || "",
      payload.initiatorType || "",
      payload.targetTag || "",
      payload.success == null ? "" : String(payload.success),
      String(payload.startTime || 0),
    ].join("\n");
    if (seen.has(key)) return;
    seen.add(key);
    post(payload);
  }};
  const performancePayload = (entry) => ({{
    event: "resource",
    url: String(entry.name || ""),
    initiatorType: String(entry.initiatorType || ""),
    targetTag: null,
    success: null,
    startTime: finite(entry.startTime),
    duration: finite(entry.duration),
    transferSize: finite(entry.transferSize),
    encodedBodySize: finite(entry.encodedBodySize),
    decodedBodySize: finite(entry.decodedBodySize),
    nextHopProtocol: nullable(entry.nextHopProtocol),
    renderBlockingStatus: nullable(entry.renderBlockingStatus),
  }});
  const elementUrl = (element) => {{
    if (!element) return "";
    return element.currentSrc || element.src || element.href || element.data || "";
  }};
  const elementInitiator = (element) => {{
    const tag = String((element && element.tagName) || "").toLowerCase();
    if (tag === "link") return String(element.rel || "link").toLowerCase();
    if (tag === "img") return "img";
    if (tag === "script") return "script";
    if (tag === "iframe") return "iframe";
    if (tag === "source") return "source";
    if (tag === "video" || tag === "audio") return tag;
    return tag;
  }};
  const elementPayload = (event) => {{
    const element = event.target && event.target.nodeType === Node.ELEMENT_NODE
      ? event.target
      : event.target && event.target.parentElement;
    const url = elementUrl(element);
    if (!url) return null;
    return {{
      event: event.type || "",
      url: String(url),
      initiatorType: elementInitiator(element),
      targetTag: element ? String(element.tagName || "").toUpperCase() : null,
      success: event.type === "load" ? true : event.type === "error" ? false : null,
      startTime: 0,
      duration: 0,
      transferSize: 0,
      encodedBodySize: 0,
      decodedBodySize: 0,
      nextHopProtocol: null,
      renderBlockingStatus: null,
    }};
  }};
  try {{
    for (const entry of performance.getEntriesByType("resource") || []) {{
      emit(performancePayload(entry));
    }}
  }} catch (_) {{}}
  if (typeof PerformanceObserver === "function") {{
    try {{
      const observer = new PerformanceObserver((list) => {{
        for (const entry of list.getEntries() || []) emit(performancePayload(entry));
      }});
      observer.observe({{ type: "resource", buffered: true }});
    }} catch (_) {{}}
  }}
  document.addEventListener("load", (event) => emit(elementPayload(event)), true);
  document.addEventListener("error", (event) => emit(elementPayload(event)), true);
}})();"#
    )
    .into()
}

/// Build a script that forwards `fetch` and `XMLHttpRequest` outcomes.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, api, method, url, status, statusText, ok, durationMs, errorName, errorMessage, responseType, documentUrl }`.
/// This is network observability for hosted JavaScript API calls; it does not
/// intercept, rewrite, or block requests.
pub fn webview_network_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"network\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelNetworkBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    try {{
      if (window.kael && typeof window.kael.post === "function") {{
        window.kael.post(kind, payload);
      }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
        window.gpui.postMessage({{ kind, payload }});
      }} else if (window.external && typeof window.external.invoke === "function") {{
        window.external.invoke({{ kind, payload }});
      }}
    }} catch (_) {{}}
  }};
  const now = () => performance && typeof performance.now === "function" ? performance.now() : Date.now();
  const duration = (started) => Math.max(0, now() - started);
  const nullable = (value) => value == null || value === "" ? null : String(value);
  const requestUrl = (input) => {{
    try {{
      if (typeof input === "string") return new URL(input, location.href).href;
      if (input && input.url) return new URL(input.url, location.href).href;
    }} catch (_) {{}}
    return String(input || "");
  }};
  const requestMethod = (input, init) => {{
    const method = init && init.method ? init.method : input && input.method ? input.method : "GET";
    return String(method || "GET").toUpperCase();
  }};
  const basePayload = (event, api, method, url, started) => ({{
    event,
    api,
    method: String(method || "GET").toUpperCase(),
    url: String(url || ""),
    status: null,
    statusText: null,
    ok: null,
    durationMs: duration(started),
    errorName: null,
    errorMessage: null,
    responseType: null,
    documentUrl: location.href || "",
  }});

  if (typeof window.fetch === "function") {{
    const originalFetch = window.fetch.bind(window);
    window.fetch = (input, init) => {{
      const started = now();
      const url = requestUrl(input);
      const method = requestMethod(input, init);
      return originalFetch(input, init).then((response) => {{
        const payload = basePayload("fetch", "fetch", method, response && response.url ? response.url : url, started);
        payload.status = response ? response.status : null;
        payload.statusText = response ? nullable(response.statusText) : null;
        payload.ok = response ? !!response.ok : null;
        post(payload);
        return response;
      }}, (error) => {{
        const payload = basePayload("fetch-error", "fetch", method, url, started);
        payload.errorName = error && error.name ? String(error.name) : "Error";
        payload.errorMessage = error && error.message ? String(error.message) : String(error || "");
        post(payload);
        throw error;
      }});
    }};
  }}

  if (typeof window.XMLHttpRequest === "function") {{
    const OriginalXHR = window.XMLHttpRequest;
    window.XMLHttpRequest = function KaelXMLHttpRequest() {{
      const xhr = new OriginalXHR();
      let method = "GET";
      let url = "";
      let started = 0;
      const originalOpen = xhr.open;
      const originalSend = xhr.send;
      xhr.open = function(methodArg, urlArg, ...rest) {{
        method = String(methodArg || "GET").toUpperCase();
        try {{ url = new URL(String(urlArg || ""), location.href).href; }} catch (_) {{ url = String(urlArg || ""); }}
        return originalOpen.call(xhr, methodArg, urlArg, ...rest);
      }};
      xhr.send = function(...args) {{
        started = now();
        return originalSend.apply(xhr, args);
      }};
      const emit = (eventName) => {{
        const payload = basePayload(eventName, "XMLHttpRequest", method, xhr.responseURL || url, started || now());
        payload.status = Number.isFinite(xhr.status) && xhr.status > 0 ? xhr.status : null;
        payload.statusText = nullable(xhr.statusText);
        payload.ok = payload.status == null ? null : payload.status >= 200 && payload.status < 300;
        payload.responseType = nullable(xhr.responseType);
        post(payload);
      }};
      xhr.addEventListener("load", () => emit("xhr"));
      xhr.addEventListener("error", () => emit("xhr-error"));
      xhr.addEventListener("abort", () => emit("xhr-abort"));
      xhr.addEventListener("timeout", () => emit("xhr-timeout"));
      return xhr;
    }};
    window.XMLHttpRequest.prototype = OriginalXHR.prototype;
  }}
}})();"#
    )
    .into()
}

/// Build a script that forwards browser dialogs and beforeunload prompts.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, message, defaultValue, result, url, defaultPrevented }`. The bridge
/// preserves browser behavior for synchronous `alert`, `confirm`, and `prompt`
/// calls; it observes but does not replace native dialog handling.
pub fn webview_dialog_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"dialog\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelDialogBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    try {{
      if (window.kael && typeof window.kael.post === "function") {{
        window.kael.post(kind, payload);
      }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
        window.gpui.postMessage({{ kind, payload }});
      }} else if (window.external && typeof window.external.invoke === "function") {{
        window.external.invoke({{ kind, payload }});
      }}
    }} catch (_) {{}}
  }};
  const payload = (event, message, defaultValue, result, defaultPrevented = false) => ({{
    event,
    message: message == null ? "" : String(message),
    defaultValue: defaultValue == null ? null : String(defaultValue),
    result,
    url: location.href || "",
    defaultPrevented: !!defaultPrevented,
  }});
  const originalAlert = window.alert && window.alert.bind ? window.alert.bind(window) : window.alert;
  const originalConfirm = window.confirm && window.confirm.bind ? window.confirm.bind(window) : window.confirm;
  const originalPrompt = window.prompt && window.prompt.bind ? window.prompt.bind(window) : window.prompt;
  if (typeof originalAlert === "function") {{
    window.alert = (message) => {{
      try {{
        const result = originalAlert(message);
        post(payload("alert", message, null, null));
        return result;
      }} catch (error) {{
        post(payload("alert", message, null, null));
        throw error;
      }}
    }};
  }}
  if (typeof originalConfirm === "function") {{
    window.confirm = (message) => {{
      const result = originalConfirm(message);
      post(payload("confirm", message, null, !!result));
      return result;
    }};
  }}
  if (typeof originalPrompt === "function") {{
    window.prompt = (message, defaultValue = "") => {{
      const result = originalPrompt(message, defaultValue);
      post(payload("prompt", message, defaultValue, result == null ? null : String(result)));
      return result;
    }};
  }}
  window.addEventListener("beforeunload", (event) => {{
    const message = event.returnValue == null ? "" : String(event.returnValue);
    post(payload("beforeunload", message, null, null, event.defaultPrevented));
  }}, true);
}})();"#
    )
    .into()
}

/// Build a script that forwards browser copy/cut/paste clipboard events.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, types, text, html, targetEditable, url, defaultPrevented }`.
/// This observes clipboard events inside hosted content without preventing the
/// browser default behavior.
pub fn webview_clipboard_event_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"clipboard\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelClipboardEventBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const editable = (target) => {{
    const element = target && target.nodeType === Node.ELEMENT_NODE ? target : target && target.parentElement;
    return !!(element && (element.isContentEditable || (element.closest && element.closest("input, textarea, [contenteditable=''], [contenteditable='true']"))));
  }};
  const readData = (data, type) => {{
    if (!data || typeof data.getData !== "function") return null;
    try {{
      const value = data.getData(type);
      return value === "" ? null : value;
    }} catch (_) {{
      return null;
    }}
  }};
  const payloadFor = (event) => {{
    const data = event.clipboardData || window.clipboardData || null;
    const types = data && data.types ? Array.from(data.types).map((type) => String(type)) : [];
    return {{
      event: event.type || "",
      types,
      text: readData(data, "text/plain"),
      html: readData(data, "text/html"),
      targetEditable: editable(event.target),
      url: location.href || "",
      defaultPrevented: !!event.defaultPrevented,
    }};
  }};
  const emit = (event) => post(payloadFor(event));
  document.addEventListener("copy", emit, true);
  document.addEventListener("cut", emit, true);
  document.addEventListener("paste", emit, true);
}})();"#
    )
    .into()
}

/// Build a script that preflights browser permission requests through Kael.
///
/// The emitted bridge request uses `kind` and a payload shaped like
/// `{ permission, permissions, api, url, origin, userGesture, details }`.
/// A response of `{ decision: "deny" }` rejects the browser API call before it
/// reaches the engine. `"allow"` and `"default"` continue to the browser's
/// native permission flow.
pub fn webview_permission_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"permission\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelPermissionBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const hasBridge = () => window.kael && typeof window.kael.invoke === "function";
  const cleanClone = (value) => {{
    if (value == null) return null;
    try {{
      return JSON.parse(JSON.stringify(value));
    }} catch (_) {{
      return String(value);
    }}
  }};
  const payloadFor = (permission, permissions, api, details) => ({{
    permission,
    permissions,
    api,
    url: location.href || "",
    origin: location.origin || "",
    userGesture: !!(navigator.userActivation && navigator.userActivation.isActive),
    details: cleanClone(details),
  }});
  const decisionValue = (response) => {{
    if (response && typeof response.decision === "string") return response.decision.toLowerCase();
    if (response && response.allow === true) return "allow";
    if (response && response.granted === true) return "allow";
    if (response && response.deny === true) return "deny";
    if (response && response.denied === true) return "deny";
    return "default";
  }};
  const requestDecision = async (permission, permissions, api, details) => {{
    if (!hasBridge()) return "default";
    try {{
      const response = await window.kael.invoke(kind, payloadFor(permission, permissions, api, details), {{ timeoutMs: 60000 }});
      return decisionValue(response);
    }} catch (_) {{
      return "default";
    }}
  }};
  const deniedError = (permission) => {{
    try {{
      return new DOMException(`Kael denied ${{permission}} permission`, "NotAllowedError");
    }} catch (_) {{
      const error = new Error(`Kael denied ${{permission}} permission`);
      error.name = "NotAllowedError";
      return error;
    }}
  }};

  if (navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia === "function") {{
    const originalGetUserMedia = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getUserMedia = async (constraints = {{}}) => {{
      const permissions = [];
      if (constraints && constraints.video) permissions.push("camera");
      if (constraints && constraints.audio) permissions.push("microphone");
      const permission = permissions.length > 1 ? "media" : (permissions[0] || "media");
      const decision = await requestDecision(permission, permissions.length ? permissions : ["media"], "mediaDevices.getUserMedia", {{ constraints }});
      if (decision === "deny") throw deniedError(permission);
      return originalGetUserMedia(constraints);
    }};
  }}

  if (navigator.mediaDevices && typeof navigator.mediaDevices.getDisplayMedia === "function") {{
    const originalGetDisplayMedia = navigator.mediaDevices.getDisplayMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getDisplayMedia = async (constraints = {{}}) => {{
      const permissions = ["display-capture"];
      if (constraints && constraints.audio) permissions.push("system-audio");
      const decision = await requestDecision("display-capture", permissions, "mediaDevices.getDisplayMedia", {{ constraints }});
      if (decision === "deny") throw deniedError("display-capture");
      return originalGetDisplayMedia(constraints);
    }};
  }}

  if (navigator.geolocation) {{
    const originalGetCurrentPosition = navigator.geolocation.getCurrentPosition && navigator.geolocation.getCurrentPosition.bind(navigator.geolocation);
    const originalWatchPosition = navigator.geolocation.watchPosition && navigator.geolocation.watchPosition.bind(navigator.geolocation);
    const originalClearWatch = navigator.geolocation.clearWatch && navigator.geolocation.clearWatch.bind(navigator.geolocation);
    const watches = new Map();
    let watchId = -1;
    const geolocationDenied = () => ({{ code: 1, message: "Kael denied geolocation permission", PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 }});
    if (originalGetCurrentPosition) {{
      navigator.geolocation.getCurrentPosition = (success, error, options) => {{
        requestDecision("geolocation", ["geolocation"], "geolocation.getCurrentPosition", {{ options }}).then((decision) => {{
          if (decision === "deny") {{
            if (typeof error === "function") error(geolocationDenied());
            return;
          }}
          originalGetCurrentPosition(success, error, options);
        }});
      }};
    }}
    if (originalWatchPosition) {{
      navigator.geolocation.watchPosition = (success, error, options) => {{
        const syntheticId = watchId--;
        requestDecision("geolocation", ["geolocation"], "geolocation.watchPosition", {{ options }}).then((decision) => {{
          if (decision === "deny") {{
            watches.delete(syntheticId);
            if (typeof error === "function") error(geolocationDenied());
            return;
          }}
          const realId = originalWatchPosition(success, error, options);
          watches.set(syntheticId, realId);
        }});
        return syntheticId;
      }};
      if (originalClearWatch) {{
        navigator.geolocation.clearWatch = (id) => {{
          const realId = watches.has(id) ? watches.get(id) : id;
          watches.delete(id);
          originalClearWatch(realId);
        }};
      }}
    }}
  }}

  if (typeof Notification !== "undefined" && typeof Notification.requestPermission === "function") {{
    const originalRequestPermission = Notification.requestPermission.bind(Notification);
    Notification.requestPermission = (callback) => {{
      const promise = requestDecision("notifications", ["notifications"], "Notification.requestPermission", null)
        .then((decision) => decision === "deny" ? "denied" : originalRequestPermission())
        .then((result) => {{
          if (typeof callback === "function") callback(result);
          return result;
        }});
      return promise;
    }};
  }}
}})();"#
    )
    .into()
}

/// Build a script that forwards Web Storage changes from hosted content.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, area, key, oldValue, newValue, length, url, local }`.
/// The bridge observes `localStorage` and `sessionStorage` mutations without
/// changing the browser's normal storage behavior.
pub fn webview_storage_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"storage\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelStorageBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    try {{
      if (window.kael && typeof window.kael.post === "function") {{
        window.kael.post(kind, payload);
      }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
        window.gpui.postMessage({{ kind, payload }});
      }} else if (window.external && typeof window.external.invoke === "function") {{
        window.external.invoke({{ kind, payload }});
      }}
    }} catch (_) {{}}
  }};
  const safeLength = (storage) => {{
    try {{
      return storage && Number.isFinite(storage.length) ? storage.length : 0;
    }} catch (_) {{
      return 0;
    }}
  }};
  const stringValue = (value) => value == null ? null : String(value);
  const payloadFor = (event, area, storage, key, oldValue, newValue, local) => ({{
    event,
    area,
    key: key == null ? null : String(key),
    oldValue: stringValue(oldValue),
    newValue: stringValue(newValue),
    length: safeLength(storage),
    url: location.href || "",
    local: !!local,
  }});
  const wrapArea = (area, storage) => {{
    if (!storage || storage.__kaelStorageWrapped) return;
    try {{
      const originalSetItem = storage.setItem.bind(storage);
      const originalRemoveItem = storage.removeItem.bind(storage);
      const originalClear = storage.clear.bind(storage);
      Object.defineProperty(storage, "__kaelStorageWrapped", {{ value: true, configurable: false }});
      storage.setItem = (key, value) => {{
        const stringKey = String(key);
        let oldValue = null;
        try {{ oldValue = storage.getItem(stringKey); }} catch (_) {{}}
        const result = originalSetItem(stringKey, value);
        let newValue = null;
        try {{ newValue = storage.getItem(stringKey); }} catch (_) {{}}
        post(payloadFor("setItem", area, storage, stringKey, oldValue, newValue, true));
        return result;
      }};
      storage.removeItem = (key) => {{
        const stringKey = String(key);
        let oldValue = null;
        try {{ oldValue = storage.getItem(stringKey); }} catch (_) {{}}
        const result = originalRemoveItem(stringKey);
        post(payloadFor("removeItem", area, storage, stringKey, oldValue, null, true));
        return result;
      }};
      storage.clear = () => {{
        const oldLength = safeLength(storage);
        const result = originalClear();
        post(payloadFor("clear", area, storage, null, oldLength ? "<cleared>" : null, null, true));
        return result;
      }};
    }} catch (_) {{}}
  }};
  try {{ wrapArea("localStorage", window.localStorage); }} catch (_) {{}}
  try {{ wrapArea("sessionStorage", window.sessionStorage); }} catch (_) {{}}
  window.addEventListener("storage", (event) => {{
    const area = event.storageArea === window.sessionStorage ? "sessionStorage" : "localStorage";
    post(payloadFor("storage", area, event.storageArea, event.key, event.oldValue, event.newValue, false));
  }});
}})();"#
    )
    .into()
}

/// Build a script that forwards favicon candidate changes through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ urls }`, where `urls` contains resolved favicon URLs in document order.
/// Inject this with [`WebViewOptions::favicon_bridge`] or
/// [`WebView::favicon_bridge`] when native tabs or breadcrumbs should update
/// as hosted pages add, remove, or change favicon links.
pub fn webview_favicon_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"favicon\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelFaviconBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const readFavicons = () => {{
    const selectors = [
      "link[rel~='icon'][href]",
      "link[rel='shortcut icon'][href]",
      "link[rel~='apple-touch-icon'][href]",
      "link[rel~='mask-icon'][href]",
    ];
    const urls = [];
    const seen = new Set();
    for (const link of Array.from(document.querySelectorAll(selectors.join(",")))) {{
      const href = link.href || link.getAttribute("href") || "";
      if (href && !seen.has(href)) {{
        seen.add(href);
        urls.push(href);
      }}
    }}
    return urls;
  }};
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  let last = "";
  const publish = () => {{
    const urls = readFavicons();
    const next = JSON.stringify(urls);
    if (next === last) return;
    last = next;
    post({{ urls }});
  }};
  publish();
  const target = document.head || document.documentElement || document;
  new MutationObserver(publish).observe(target, {{ childList: true, subtree: true, attributes: true, attributeFilter: ["href", "rel"] }});
  window.addEventListener("pageshow", publish);
}})();"#
    )
    .into()
}

/// Build a script that forwards page console output through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ level, message, args, source, line, column }`. Inject this with
/// [`WebViewOptions::console_bridge`] or [`WebView::console_bridge`] when
/// native diagnostics, test harnesses, or AI agents should observe hosted page
/// logs without opening browser devtools.
pub fn webview_console_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"console\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelConsoleBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const safeValue = (value) => {{
    if (value instanceof Error) {{
      return {{ name: value.name || "Error", message: value.message || "", stack: value.stack || null }};
    }}
    if (value === undefined) return null;
    if (typeof value === "function") return String(value);
    try {{
      return JSON.parse(JSON.stringify(value));
    }} catch (_) {{
      return String(value);
    }}
  }};
  const textValue = (value) => {{
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.stack || value.message || String(value);
    try {{
      const json = JSON.stringify(value);
      return json === undefined ? String(value) : json;
    }} catch (_) {{
      return String(value);
    }}
  }};
  const emit = (level, args, line = null, column = null) => {{
    const values = Array.from(args || []);
    post({{
      level,
      message: values.map(textValue).join(" "),
      args: values.map(safeValue),
      source: location.href || null,
      line,
      column,
    }});
  }};
  const methods = ["debug", "log", "info", "warn", "error"];
  for (const level of methods) {{
    const original = console[level] && console[level].bind ? console[level].bind(console) : console[level];
    if (typeof original !== "function") continue;
    console[level] = (...args) => {{
      try {{ emit(level, args); }} catch (_) {{}}
      return original(...args);
    }};
  }}
  window.addEventListener("error", (event) => {{
    emit("error", [event.error || event.message || "Script error"], event.lineno || null, event.colno || null);
  }});
  window.addEventListener("unhandledrejection", (event) => {{
    emit("error", [event.reason || "Unhandled promise rejection"]);
  }});
}})();"#
    )
    .into()
}

/// Build a script that forwards browser keyboard/input events through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, key, code, location, repeat, isComposing, altKey, ctrlKey,
/// metaKey, shiftKey, targetEditable, inputType, data, defaultPrevented }`.
/// Inject this with [`WebViewOptions::keyboard_event_bridge`] or
/// [`WebView::keyboard_event_bridge`] when native chrome or agents need to
/// observe hosted editor shortcuts and before-input activity.
pub fn webview_keyboard_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"keyboard\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelKeyboardBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  const editable = (target) => {{
    const element = target && target.nodeType === Node.ELEMENT_NODE ? target : target && target.parentElement;
    return !!(element && (element.isContentEditable || (element.closest && element.closest("input, textarea, [contenteditable=''], [contenteditable='true']"))));
  }};
  const keyboardPayload = (event) => ({{
    event: event.type,
    key: event.key || null,
    code: event.code || null,
    location: event.location || 0,
    repeat: !!event.repeat,
    isComposing: !!event.isComposing,
    altKey: !!event.altKey,
    ctrlKey: !!event.ctrlKey,
    metaKey: !!event.metaKey,
    shiftKey: !!event.shiftKey,
    targetEditable: editable(event.target),
    inputType: null,
    data: null,
    defaultPrevented: !!event.defaultPrevented,
  }});
  const inputPayload = (event) => ({{
    event: event.type,
    key: null,
    code: null,
    location: 0,
    repeat: false,
    isComposing: !!event.isComposing,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    targetEditable: editable(event.target),
    inputType: event.inputType || null,
    data: event.data || null,
    defaultPrevented: !!event.defaultPrevented,
  }});
  document.addEventListener("keydown", (event) => post(keyboardPayload(event)), true);
  document.addEventListener("keyup", (event) => post(keyboardPayload(event)), true);
  document.addEventListener("beforeinput", (event) => post(inputPayload(event)), true);
}})();"#
    )
    .into()
}

/// Build a script that forwards same-document location changes through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ url, title, readyState, canGoBack, canGoForward }`. Inject this with
/// [`WebViewOptions::location_bridge`] or [`WebView::location_bridge`] when
/// native tabs, breadcrumbs, loading UI, or agents should observe hosted SPA
/// route changes without polling.
pub fn webview_location_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"location\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelLocationBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  let lastKey = "";
  const navigationState = () => window.__kaelNavigationState || window.kaelNavigationState || null;
  const payload = () => {{
    const state = navigationState();
    return {{
      url: location.href || "",
      title: document.title || "",
      readyState: document.readyState || "",
      canGoBack: !!(state ? state.canGoBack : history.length > 1),
      canGoForward: !!(state && state.canGoForward),
    }};
  }};
  const emit = () => {{
    const current = payload();
    const key = JSON.stringify(current);
    if (key === lastKey) return;
    lastKey = key;
    post(current);
  }};
  const wrapHistory = (name) => {{
    const original = history[name];
    if (typeof original !== "function" || original.__kaelLocationWrapped) return;
    const wrapped = function(...args) {{
      const result = original.apply(this, args);
      queueMicrotask(emit);
      return result;
    }};
    wrapped.__kaelLocationWrapped = true;
    history[name] = wrapped;
  }};
  wrapHistory("pushState");
  wrapHistory("replaceState");
  window.addEventListener("popstate", emit);
  window.addEventListener("hashchange", emit);
  window.addEventListener("pageshow", emit);
  document.addEventListener("DOMContentLoaded", emit);
  window.addEventListener("load", emit);
  new MutationObserver(emit).observe(document.documentElement || document, {{ childList: true, subtree: true }});
  emit();
}})();"#
    )
    .into()
}

/// Build a script that forwards browser lifecycle/focus events through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, visibilityState, hidden, hasFocus, fullscreen, persisted }`.
/// Inject this with [`WebViewOptions::lifecycle_bridge`] or
/// [`WebView::lifecycle_bridge`] when native chrome, resource throttling, tests,
/// or agents should observe whether hosted content is active or visible.
pub fn webview_lifecycle_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"lifecycle\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelLifecycleBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  let lastKey = "";
  const isFullscreen = () => !!(
    document.fullscreenElement ||
    document.webkitFullscreenElement ||
    document.msFullscreenElement
  );
  const payload = (event, persisted = null) => ({{
    event,
    visibilityState: document.visibilityState || "",
    hidden: !!document.hidden,
    hasFocus: typeof document.hasFocus === "function" ? document.hasFocus() : false,
    fullscreen: isFullscreen(),
    persisted,
  }});
  const emit = (event, persisted = null) => {{
    const current = payload(event, persisted);
    const key = JSON.stringify(current);
    if (key === lastKey) return;
    lastKey = key;
    post(current);
  }};
  window.addEventListener("focus", () => emit("focus"), true);
  window.addEventListener("blur", () => emit("blur"), true);
  document.addEventListener("visibilitychange", () => emit("visibilitychange"), true);
  window.addEventListener("pageshow", (event) => emit("pageshow", !!event.persisted), true);
  window.addEventListener("pagehide", (event) => emit("pagehide", !!event.persisted), true);
  document.addEventListener("fullscreenchange", () => emit("fullscreenchange"), true);
  document.addEventListener("webkitfullscreenchange", () => emit("fullscreenchange"), true);
  document.addEventListener("MSFullscreenChange", () => emit("fullscreenchange"), true);
  emit("initial");
}})();"#
    )
    .into()
}

/// Build a script that forwards browser scroll/viewport snapshots through `window.kael`.
///
/// The emitted bridge message uses `kind` and a payload shaped like
/// `{ event, x, y, maxX, maxY, viewportWidth, viewportHeight, scrollWidth,
/// scrollHeight, progressX, progressY }`. Inject this with
/// [`WebViewOptions::scroll_bridge`] or [`WebView::scroll_bridge`] when native
/// chrome, reader progress, tests, or agents need to observe hosted document
/// scroll state without polling.
pub fn webview_scroll_bridge_script(kind: impl AsRef<str>) -> SharedString {
    let kind = serde_json::to_string(kind.as_ref()).unwrap_or_else(|_| "\"scroll\"".into());
    format!(
        r#"(() => {{
  const kind = {kind};
  const bridgeKey = "__kaelScrollBridge";
  if (!window[bridgeKey]) window[bridgeKey] = new Set();
  if (window[bridgeKey].has(kind)) return;
  window[bridgeKey].add(kind);
  const post = (payload) => {{
    if (window.kael && typeof window.kael.post === "function") {{
      window.kael.post(kind, payload);
    }} else if (window.gpui && typeof window.gpui.postMessage === "function") {{
      window.gpui.postMessage({{ kind, payload }});
    }} else if (window.external && typeof window.external.invoke === "function") {{
      window.external.invoke({{ kind, payload }});
    }}
  }};
  let scheduled = false;
  let pendingEvent = "initial";
  let lastKey = "";
  const number = (value) => Number.isFinite(value) ? value : 0;
  const snapshot = (event) => {{
    const root = document.scrollingElement || document.documentElement || document.body;
    const body = document.body || root;
    const viewportWidth = number(window.innerWidth || document.documentElement.clientWidth || 0);
    const viewportHeight = number(window.innerHeight || document.documentElement.clientHeight || 0);
    const scrollWidth = number(Math.max(root ? root.scrollWidth : 0, body ? body.scrollWidth : 0, viewportWidth));
    const scrollHeight = number(Math.max(root ? root.scrollHeight : 0, body ? body.scrollHeight : 0, viewportHeight));
    const maxX = Math.max(0, scrollWidth - viewportWidth);
    const maxY = Math.max(0, scrollHeight - viewportHeight);
    const x = Math.min(Math.max(0, number(window.scrollX || (root && root.scrollLeft) || 0)), maxX);
    const y = Math.min(Math.max(0, number(window.scrollY || (root && root.scrollTop) || 0)), maxY);
    return {{
      event,
      x,
      y,
      maxX,
      maxY,
      viewportWidth,
      viewportHeight,
      scrollWidth,
      scrollHeight,
      progressX: maxX > 0 ? x / maxX : 0,
      progressY: maxY > 0 ? y / maxY : 0,
    }};
  }};
  const flush = () => {{
    scheduled = false;
    const current = snapshot(pendingEvent);
    const key = JSON.stringify(current);
    if (key === lastKey) return;
    lastKey = key;
    post(current);
  }};
  const schedule = (event) => {{
    pendingEvent = event;
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(flush);
  }};
  window.addEventListener("scroll", () => schedule("scroll"), {{ passive: true, capture: true }});
  window.addEventListener("resize", () => schedule("resize"), true);
  document.addEventListener("DOMContentLoaded", () => schedule("resize"));
  window.addEventListener("load", () => schedule("resize"));
  schedule("initial");
}})();"#
    )
    .into()
}

fn webview_is_loading_script() -> SharedString {
    "document.readyState !== \"complete\"".into()
}

fn webview_can_go_back_script() -> SharedString {
    "history.length > 1".into()
}

fn webview_can_go_forward_script() -> SharedString {
    r#"(() => {
  const state = window.__kaelNavigationState || window.kaelNavigationState;
  if (state && typeof state.canGoForward === "boolean") {
    return state.canGoForward;
  }
  return false;
})()"#
        .into()
}

/// JavaScript helper that tracks same-document navigation stack state.
///
/// Inject this with [`WebViewOptions::navigation_state_bridge`] or
/// [`WebView::navigation_state_bridge`] when native chrome should enable or
/// disable Forward buttons for app-owned WebView islands. Browser engines do
/// not expose their forward stack to page JavaScript, so this helper tracks
/// `pushState`, `replaceState`, and `popstate` entries created after injection.
pub fn webview_navigation_state_bridge_script() -> SharedString {
    r#"
(() => {
  if (window.__kaelNavigationState && window.__kaelNavigationState.__bridgeVersion) return;
  const KEY = "__kaelNavigationIndex";
  const wrap = (state, index) => {
    const base = state && typeof state === "object" && !Array.isArray(state) ? state : { value: state };
    return { ...base, [KEY]: index };
  };
  const readIndex = (state) => {
    if (state && typeof state === "object" && Number.isFinite(state[KEY])) return state[KEY];
    return null;
  };
  let index = readIndex(history.state);
  if (index == null) index = 0;
  let max = index;
  const publish = () => {
    window.__kaelNavigationState = window.kaelNavigationState = {
      __bridgeVersion: 1,
      index,
      max,
      canGoBack: index > 0 || history.length > 1,
      canGoForward: index < max,
    };
  };
  const replaceCurrent = () => {
    try {
      history.replaceState(wrap(history.state, index), document.title, location.href);
    } catch (_) {}
  };
  replaceCurrent();
  publish();
  const originalPushState = history.pushState.bind(history);
  const originalReplaceState = history.replaceState.bind(history);
  history.pushState = (state, title, url) => {
    index += 1;
    max = index;
    const result = originalPushState(wrap(state, index), title, url);
    publish();
    return result;
  };
  history.replaceState = (state, title, url) => {
    const result = originalReplaceState(wrap(state, index), title, url);
    publish();
    return result;
  };
  window.addEventListener("popstate", (event) => {
    const next = readIndex(event.state);
    if (next != null) index = next;
    publish();
  });
})();"#
        .into()
}

fn parse_webview_bool_result(value: &str) -> Result<bool, SharedString> {
    match serde_json::from_str::<bool>(value) {
        Ok(value) => Ok(value),
        Err(_) if value == "true" => Ok(true),
        Err(_) if value == "false" => Ok(false),
        Err(_) => Err(format!("WebView returned a non-boolean result: {value}").into()),
    }
}

fn parse_webview_string_result(value: &str) -> Result<SharedString, SharedString> {
    match serde_json::from_str::<String>(value) {
        Ok(value) => Ok(value.into()),
        Err(_) if value == "null" => Ok(SharedString::default()),
        Err(_) => Err(format!("WebView returned a non-string result: {value}").into()),
    }
}

fn parse_webview_optional_string_result(value: &str) -> Result<Option<SharedString>, SharedString> {
    match serde_json::from_str::<Option<String>>(value) {
        Ok(value) => Ok(value.map(SharedString::from)),
        Err(_) => Err(format!("WebView returned a non-optional-string result: {value}").into()),
    }
}

fn parse_webview_string_array_result(value: &str) -> Result<Vec<SharedString>, SharedString> {
    let values: Vec<String> = serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-string-array result: {value}"))?;
    Ok(values.into_iter().map(SharedString::from).collect())
}

fn parse_webview_scroll_event_result(value: &str) -> Result<WebViewScrollEvent, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-scroll-snapshot result: {value}").into())
}

fn parse_webview_optional_scroll_event_result(
    value: &str,
) -> Result<Option<WebViewScrollEvent>, SharedString> {
    serde_json::from_str(value).map_err(|_| {
        format!("WebView returned a non-optional-scroll-snapshot result: {value}").into()
    })
}

fn parse_webview_find_result(value: &str) -> Result<WebViewFindResult, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-find-result value: {value}").into())
}

fn parse_webview_document_snapshot_result(
    value: &str,
) -> Result<WebViewDocumentSnapshot, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-document-snapshot result: {value}").into())
}

fn parse_webview_element_snapshot_result(
    value: &str,
) -> Result<Option<WebViewElementSnapshot>, SharedString> {
    serde_json::from_str(value).map_err(|_| {
        format!("WebView returned a non-optional-element-snapshot result: {value}").into()
    })
}

fn parse_webview_download_trigger_result(
    value: &str,
) -> Result<WebViewDownloadTriggerResult, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-download-trigger result: {value}").into())
}

fn parse_webview_storage_snapshot_result(
    value: &str,
) -> Result<WebViewStorageSnapshot, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-storage-snapshot result: {value}").into())
}

fn parse_webview_storage_mutation_result(
    value: &str,
) -> Result<WebViewStorageMutationResult, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-storage-mutation result: {value}").into())
}

fn parse_webview_media_state_result(
    value: &str,
) -> Result<Vec<WebViewMediaElementState>, SharedString> {
    serde_json::from_str(value)
        .map_err(|_| format!("WebView returned a non-media-state result: {value}").into())
}

/// JavaScript helper injected into a WebView island for bridge messaging.
///
/// It exposes `window.kael.post(kind, payload, id)`,
/// `window.kael.invoke(kind, payload, options)`,
/// `window.kael.onMessage(handler)`, and `window.kael.offMessage(handler)`.
/// Host-to-WebView messages sent with [`WebViewController::post_bridge_message`]
/// are delivered through the standard browser `message` event and forwarded to
/// registered `window.kael` handlers.
pub fn webview_bridge_script() -> SharedString {
    r#"
(() => {
  if (window.kael && window.kael.__bridgeVersion) return;
  const listeners = new Set();
  const pending = new Map();
  let counter = 0;
  const normalize = (kind, payload, id) => {
    if (typeof kind === "object" && kind !== null) return kind;
    const message = { kind: String(kind), payload: payload === undefined ? null : payload };
    if (id !== undefined && id !== null) message.id = String(id);
    return message;
  };
  const nextId = () => `kael-${Date.now()}-${++counter}`;
  const post = (kind, payload, id) => {
    const message = normalize(kind, payload, id);
    if (window.gpui && typeof window.gpui.postMessage === "function") {
      window.gpui.postMessage(message);
    } else if (window.external && typeof window.external.invoke === "function") {
      window.external.invoke(message);
    } else {
      throw new Error("Kael WebView bridge is not available");
    }
    return message;
  };
  window.kael = {
    __bridgeVersion: 2,
    post(kind, payload, id) {
      post(kind, payload, id);
    },
    invoke(kind, payload, options = {}) {
      const id = options.id == null ? nextId() : String(options.id);
      const timeoutMs = options.timeoutMs == null ? 30000 : Number(options.timeoutMs);
      return new Promise((resolve, reject) => {
        const timeout = timeoutMs > 0
          ? setTimeout(() => {
              pending.delete(id);
              reject(new Error(`Kael WebView bridge request timed out: ${kind}`));
            }, timeoutMs)
          : null;
        pending.set(id, { kind: String(kind), resolve, reject, timeout });
        try {
          post(kind, payload, id);
        } catch (error) {
          if (timeout) clearTimeout(timeout);
          pending.delete(id);
          reject(error);
        }
      });
    },
    onMessage(handler) {
      listeners.add(handler);
      return () => listeners.delete(handler);
    },
    offMessage(handler) {
      listeners.delete(handler);
    }
  };
  window.addEventListener("message", (event) => {
    const message = event.data;
    if (message && message.id != null && pending.has(String(message.id))) {
      const entry = pending.get(String(message.id));
      const kind = String(message.kind || "");
      const isResponse = kind === "response" || kind === `${entry.kind}:response`;
      const isError = kind === "error" || kind === `${entry.kind}:error`;
      if (isResponse || isError) {
        pending.delete(String(message.id));
        if (entry.timeout) clearTimeout(entry.timeout);
        if (isError) {
          const detail = message.payload && message.payload.message ? message.payload.message : kind;
          entry.reject(new Error(String(detail)));
        } else {
          entry.resolve(message.payload);
        }
      }
    }
    for (const handler of Array.from(listeners)) handler(message);
  });
})();
"#
    .into()
}

/// Configuration for a WebView island.
///
/// Use this when a WebView is an intentional compatibility boundary for an
/// auth flow, hosted payment, embedded document, third-party widget, or
/// browser-only graphics/media surface. The existing fluent methods on
/// [`WebView`] remain available for small inline cases.
#[derive(Clone, Default)]
pub struct WebViewOptions {
    storage_key: Option<SharedString>,
    user_agent: Option<SharedString>,
    injected_css: Vec<SharedString>,
    injected_javascript: Vec<SharedString>,
    html: Option<SharedString>,
    request_headers: Option<http_client::http::HeaderMap>,
    javascript_disabled: bool,
    general_autofill: Option<bool>,
    background_color: Option<Rgba>,
    devtools: bool,
    zoom_hotkeys_enabled: bool,
    media_autoplay: Option<bool>,
    focused: Option<bool>,
    clipboard_access: bool,
    on_message: Option<WebViewMessageHandler>,
    on_navigate: Option<WebViewNavigationHandler>,
    on_new_window: Option<WebViewNewWindowHandler>,
    on_download_started: Option<WebViewDownloadStartedHandler>,
    on_download_completed: Option<WebViewDownloadCompletedHandler>,
    on_document_title_changed: Option<WebViewDocumentTitleChangedHandler>,
    on_page_load: Option<WebViewPageLoadHandler>,
    on_drag_drop: Option<WebViewDragDropHandler>,
    on_permission_request: Option<(SharedString, WebViewPermissionRequestHandler)>,
}

impl WebViewOptions {
    /// Create empty WebView options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Persistent profile preset for auth, OAuth, SSO, and hosted account flows.
    pub fn auth_flow(storage_key: impl Into<SharedString>) -> Self {
        Self::new()
            .storage_key(storage_key)
            .inject_css("html, body { overscroll-behavior: none; }")
            .allow_navigation_schemes(["https", "http"])
    }

    /// Ephemeral preset for payments, embeds, and third-party widgets.
    pub fn embedded_widget() -> Self {
        Self::new()
            .inject_css("html, body { overscroll-behavior: none; }")
            .allow_navigation_schemes(["https", "http", "data", "about", "blob"])
    }

    /// Ephemeral preset for browser-only WebGL/WebGPU/canvas surfaces.
    pub fn web_graphics() -> Self {
        Self::embedded_widget().inject_css(
            "html, body, canvas { margin: 0; width: 100%; height: 100%; overflow: hidden; }",
        )
    }

    /// Enable WebView developer tools for development.
    pub fn devtools(self) -> Self {
        self.devtools_enabled(true)
    }

    /// Enable or disable WebView developer tools.
    pub fn devtools_enabled(mut self, enabled: bool) -> Self {
        self.devtools = enabled;
        self
    }

    /// Enable browser zoom hotkeys and gestures where the backend supports them.
    pub fn zoom_hotkeys(self) -> Self {
        self.zoom_hotkeys_enabled(true)
    }

    /// Enable or disable browser zoom hotkeys and gestures.
    pub fn zoom_hotkeys_enabled(mut self, enabled: bool) -> Self {
        self.zoom_hotkeys_enabled = enabled;
        self
    }

    /// Allow all WebView media to autoplay without user interaction.
    pub fn media_autoplay(self) -> Self {
        self.media_autoplay_enabled(true)
    }

    /// Enable or disable WebView media autoplay without user interaction.
    pub fn media_autoplay_enabled(mut self, enabled: bool) -> Self {
        self.media_autoplay = Some(enabled);
        self
    }

    /// Request that this WebView receive focus when created.
    pub fn focused(self) -> Self {
        self.focused_enabled(true)
    }

    /// Enable or disable initial WebView focus when the backend supports it.
    pub fn focused_enabled(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// Enable JavaScript clipboard access for pages that use browser clipboard APIs.
    pub fn clipboard_access(self) -> Self {
        self.clipboard_access_enabled(true)
    }

    /// Enable or disable JavaScript clipboard access where the backend supports it.
    pub fn clipboard_access_enabled(mut self, enabled: bool) -> Self {
        self.clipboard_access = enabled;
        self
    }

    /// Add request headers for the WebView's initial URL load.
    pub fn request_headers(mut self, headers: http_client::http::HeaderMap) -> Self {
        self.request_headers = Some(headers);
        self
    }

    /// Remove request headers from the WebView's initial URL load.
    pub fn clear_request_headers(mut self) -> Self {
        self.request_headers = None;
        self
    }

    /// Disable JavaScript for untrusted or static browser content.
    pub fn javascript_disabled(self) -> Self {
        self.javascript_disabled_enabled(true)
    }

    /// Enable or disable JavaScript execution where the backend supports it.
    pub fn javascript_disabled_enabled(mut self, disabled: bool) -> Self {
        self.javascript_disabled = disabled;
        self
    }

    /// Disable browser-level general autofill where the backend supports it.
    pub fn general_autofill_disabled(self) -> Self {
        self.general_autofill_enabled(false)
    }

    /// Enable or disable browser-level general autofill where the backend supports it.
    pub fn general_autofill_enabled(mut self, enabled: bool) -> Self {
        self.general_autofill = Some(enabled);
        self
    }

    /// Set the native WebView background color.
    pub fn background_color(mut self, color: impl Into<Rgba>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Request a transparent native WebView background.
    pub fn transparent_background(self) -> Self {
        self.background_color(Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        })
    }

    /// Use persistent storage under the supplied profile key.
    pub fn storage_key(mut self, key: impl Into<SharedString>) -> Self {
        self.storage_key = Some(key.into());
        self
    }

    /// Override the user agent used by the platform WebView.
    pub fn user_agent(mut self, user_agent: impl Into<SharedString>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Inject CSS into each loaded page.
    pub fn inject_css(mut self, css: impl Into<SharedString>) -> Self {
        self.injected_css.push(css.into());
        self
    }

    /// Inject JavaScript into each loaded page.
    pub fn inject_javascript(mut self, javascript: impl Into<SharedString>) -> Self {
        self.injected_javascript.push(javascript.into());
        self
    }

    /// Load an HTML string as the WebView's initial document when no URL is supplied.
    pub fn html(mut self, html: impl Into<SharedString>) -> Self {
        self.html = Some(html.into());
        self
    }

    /// Clear the initial HTML document configured for this WebView.
    pub fn clear_html(mut self) -> Self {
        self.html = None;
        self
    }

    /// Register a handler for messages posted from JavaScript.
    pub fn on_message(
        mut self,
        handler: impl Fn(serde_json::Value, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_message = Some(Rc::new(handler));
        self
    }

    /// Register a handler for typed WebView bridge envelopes.
    pub fn on_bridge_message(
        mut self,
        handler: impl Fn(WebViewBridgeMessage, &mut Window, &mut App) + 'static,
    ) -> Self {
        let previous = self.on_message.take();
        let handler = Rc::new(handler);
        self.on_message = Some(Rc::new(move |value, window, cx| {
            if let Some(previous) = previous.as_ref() {
                previous(value.clone(), window, cx);
            }
            if let Some(message) = WebViewBridgeMessage::from_value(value) {
                handler(message, window, cx);
            }
        }));
        self
    }

    /// Inject the standard `window.kael` bridge helper.
    pub fn bridge_script(self) -> Self {
        self.inject_javascript(webview_bridge_script())
    }

    /// Inject same-document navigation state tracking for app-owned pages.
    ///
    /// This tracks `pushState`, `replaceState`, and `popstate` entries created
    /// after injection and publishes `window.__kaelNavigationState` for
    /// [`WebViewController::can_go_forward`].
    pub fn navigation_state_bridge(self) -> Self {
        self.inject_javascript(webview_navigation_state_bridge_script())
    }

    /// Inject the standard bridge plus native desktop find-result forwarding.
    ///
    /// The injected script wraps `window.find(...)` and posts
    /// [`WebViewBridgeMessage`] values with the given kind after browser find
    /// operations run. Use this for native find bars that need `found-in-page`
    /// style updates without polling JavaScript.
    pub fn find_result_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_find_result_bridge_script(kind))
    }

    /// Inject find-result forwarding and handle typed find events.
    ///
    /// This observes app-driven browser find calls, including
    /// [`WebViewController::find_text`] and [`WebViewController::find_text_result`]
    /// when the bridge is injected before those commands are evaluated.
    pub fn on_find_result(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFindEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.find_result_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) = WebViewFindEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser media event forwarding.
    ///
    /// The injected script posts `WebViewBridgeMessage` values with the given
    /// kind whenever current or future `<audio>` / `<video>` elements emit
    /// common playback, timing, volume, buffering, or error events.
    pub fn media_event_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_media_event_bridge_script(kind))
    }

    /// Inject browser media event forwarding and handle typed media events.
    ///
    /// This is the shortest path for custom native controls around
    /// WebView-hosted `<audio>` / `<video>` content: the bridge posts browser
    /// media events and this handler receives parsed [`WebViewMediaEvent`]
    /// values for the requested kind.
    pub fn on_media_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewMediaEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.media_event_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) = WebViewMediaEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser context-menu forwarding.
    ///
    /// The injected script prevents the browser default context menu and posts
    /// [`WebViewBridgeMessage`] values with the given kind for right-click /
    /// secondary-click events inside WebView content.
    pub fn context_menu_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_context_menu_bridge_script(kind))
    }

    /// Inject browser context-menu forwarding and handle typed context events.
    ///
    /// Use this when native chrome should build an app menu from the clicked
    /// page context, such as selected text, links, images, media, or editable
    /// fields inside hosted browser content.
    pub fn on_context_menu(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewContextMenuEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.context_menu_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewContextMenuEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser pointer context forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for pointer movement and click-style events inside hosted content.
    pub fn pointer_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_pointer_bridge_script(kind))
    }

    /// Inject browser pointer context forwarding and handle typed pointer events.
    ///
    /// Use this when native status bars, hover previews, tests, or agents need
    /// lightweight link/image/media/editable context for hovered or clicked
    /// browser content.
    pub fn on_pointer_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewPointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.pointer_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewPointerEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser form activity forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for hosted submit, reset, change, and input events. Password and
    /// file input values are intentionally omitted from payloads.
    pub fn form_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_form_bridge_script(kind))
    }

    /// Inject browser form forwarding and handle typed form events.
    ///
    /// Use this when native validation, progress chrome, tests, or agents need
    /// structured form metadata and field state from hosted checkout, auth,
    /// settings, or admin pages without bespoke page JavaScript.
    pub fn on_form_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFormEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.form_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) = WebViewFormEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser file-input selection forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind whenever hosted `<input type="file">` controls change. Payloads
    /// include browser-exposed file metadata, not local filesystem paths.
    pub fn file_input_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_file_input_bridge_script(kind))
    }

    /// Inject file-input forwarding and handle typed file-input events.
    ///
    /// Use this when native upload chrome, tests, or agents need to observe
    /// browser file selections inside hosted forms without custom page
    /// JavaScript. Browsers do not expose selected local paths.
    pub fn on_file_input_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFileInputEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.file_input_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewFileInputEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser resource observability.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for PerformanceResourceTiming entries and element load/error
    /// events. This does not intercept or rewrite requests.
    pub fn resource_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_resource_bridge_script(kind))
    }

    /// Inject resource forwarding and handle typed resource events.
    ///
    /// Use this when native diagnostics, loading UI, tests, or agents should
    /// observe hosted subresource activity without opening devtools.
    pub fn on_resource_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewResourceEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.resource_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewResourceEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus hosted fetch/XHR network observability.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind when hosted JavaScript `fetch` and `XMLHttpRequest` calls complete,
    /// error, abort, or time out. This does not intercept or rewrite requests.
    pub fn network_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_network_bridge_script(kind))
    }

    /// Inject fetch/XHR forwarding and handle typed network events.
    ///
    /// Use this when native diagnostics, loading UI, tests, or agents need API
    /// request outcomes without opening browser devtools.
    pub fn on_network_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewNetworkEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.network_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewNetworkEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser dialog observability.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for `alert`, `confirm`, `prompt`, and `beforeunload` activity while
    /// preserving browser dialog behavior.
    pub fn dialog_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_dialog_bridge_script(kind))
    }

    /// Inject browser dialog forwarding and handle typed dialog events.
    ///
    /// Use this when native diagnostics, tests, or agents should observe hosted
    /// blocking dialogs and unload prompts. This does not replace the browser's
    /// synchronous dialog result path.
    pub fn on_dialog_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewDialogEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.dialog_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewDialogEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser clipboard event forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for hosted `copy`, `cut`, and `paste` events. This is an explicit
    /// opt-in event observer and does not prevent browser defaults.
    pub fn clipboard_event_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_clipboard_event_bridge_script(kind))
    }

    /// Inject clipboard event forwarding and handle typed clipboard events.
    ///
    /// Use this when hosted rich editors, tests, or agents need to observe
    /// copy/cut/paste activity and browser-exposed clipboard data.
    pub fn on_clipboard_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewClipboardEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.clipboard_event_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewClipboardEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser permission preflighting.
    ///
    /// The injected script wraps browser permission-bearing APIs such as
    /// `getUserMedia`, geolocation, and notifications. A denied Kael response
    /// blocks the page API call before it reaches the embedded browser; allow
    /// and default responses continue to the browser's native permission flow.
    pub fn permission_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_permission_bridge_script(kind))
    }

    /// Inject permission preflighting and handle typed permission requests.
    ///
    /// This is the WebView-island equivalent of an browser-runtime stack permission request
    /// handler for browser APIs exposed inside hosted content. The embedded
    /// browser remains the final authority for native permission prompts.
    pub fn on_permission_request(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewPermissionRequest, &mut Window, &mut App) -> WebViewPermissionDecision
        + 'static,
    ) -> Self {
        let kind = kind.into();
        self = self.permission_bridge(kind.as_ref());
        self.on_permission_request = Some((kind, Rc::new(handler)));
        self
    }

    /// Inject the standard bridge plus Web Storage event forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind when hosted content mutates `localStorage` or `sessionStorage`, and
    /// when the browser emits cross-document `storage` events.
    pub fn storage_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_storage_bridge_script(kind))
    }

    /// Inject Web Storage forwarding and handle typed storage events.
    ///
    /// Use this when native account chrome, settings sync, tests, or agents
    /// should observe hosted storage state changes without polling JavaScript.
    pub fn on_storage_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewStorageEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.storage_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewStorageEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser favicon change forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind whenever favicon link candidates in the hosted document change.
    pub fn favicon_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_favicon_bridge_script(kind))
    }

    /// Inject browser favicon forwarding and handle typed favicon events.
    ///
    /// Use this when native tabs, breadcrumbs, or history UI should react to
    /// hosted page icon changes without polling.
    pub fn on_favicon_changed(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFaviconEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.favicon_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewFaviconEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser console forwarding.
    ///
    /// The injected script preserves normal console behavior while posting
    /// [`WebViewBridgeMessage`] values with the given kind for `console.*`,
    /// `error`, and `unhandledrejection` events in the hosted page.
    pub fn console_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_console_bridge_script(kind))
    }

    /// Inject browser console forwarding and handle typed console events.
    ///
    /// Use this for app-owned diagnostics, automated tests, hosted-widget
    /// health checks, and AI-agent observability without opening browser
    /// devtools.
    pub fn on_console_message(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewConsoleEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.console_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewConsoleEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser keyboard/input forwarding.
    ///
    /// This observes `keydown`, `keyup`, and `beforeinput` events inside hosted
    /// content for native shortcut chrome, test harnesses, and agent
    /// observability.
    pub fn keyboard_event_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_keyboard_bridge_script(kind))
    }

    /// Inject browser keyboard/input forwarding and handle typed events.
    ///
    /// This is the closest portable WebView island equivalent to browser-runtime stack
    /// `before-input-event` today. It observes events after browser dispatch
    /// starts; use native shortcut systems for commands that must cancel input
    /// before the page sees it.
    pub fn on_keyboard_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewKeyboardEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.keyboard_event_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewKeyboardEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus same-document location forwarding.
    ///
    /// This observes browser route changes inside hosted content and posts
    /// [`WebViewBridgeMessage`] values with the given kind for native tabs,
    /// breadcrumbs, app-owned Back/Forward buttons, and agent observability.
    pub fn location_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_location_bridge_script(kind))
    }

    /// Inject location forwarding and handle typed location events.
    ///
    /// Use this when native chrome should track hosted SPA route changes
    /// without polling `url(...)` and `title(...)`. Pair with
    /// [`Self::navigation_state_bridge`] for same-document Forward state.
    pub fn on_location_changed(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewLocationEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.location_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewLocationEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser lifecycle/focus forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for hosted page focus, blur, visibility, page show/hide, and
    /// fullscreen changes.
    pub fn lifecycle_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_lifecycle_bridge_script(kind))
    }

    /// Inject lifecycle/focus forwarding and handle typed lifecycle events.
    ///
    /// Use this when native chrome, resource throttling, tests, or agents need
    /// to know whether hosted WebView content is focused, visible, active, or
    /// browser-fullscreen.
    pub fn on_lifecycle_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewLifecycleEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.lifecycle_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewLifecycleEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser scroll/viewport forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for hosted document scroll and viewport resize snapshots.
    pub fn scroll_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_scroll_bridge_script(kind))
    }

    /// Inject scroll/viewport forwarding and handle typed scroll events.
    ///
    /// Use this when native chrome, reader progress, tests, or agents should
    /// observe hosted document scroll state without polling JavaScript.
    pub fn on_scroll_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewScrollEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.scroll_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewScrollEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Inject the standard bridge plus browser selection forwarding.
    ///
    /// The injected script posts [`WebViewBridgeMessage`] values with the given
    /// kind for hosted document and input selection snapshots.
    pub fn selection_bridge(self, kind: impl AsRef<str>) -> Self {
        self.bridge_script()
            .inject_javascript(webview_selection_bridge_script(kind))
    }

    /// Inject selection forwarding and handle typed selection events.
    ///
    /// Use this when native edit menus, floating formatting chrome, tests, or
    /// agents should observe hosted selection state without polling JavaScript.
    pub fn on_selection_event(
        self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewSelectionEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let kind = kind.into();
        let handler = Rc::new(handler);
        self.selection_bridge(kind.as_ref())
            .on_bridge_message(move |message, window, cx| {
                if let Some(event) =
                    WebViewSelectionEvent::from_bridge_message(&message, kind.as_ref())
                {
                    handler(event, window, cx);
                }
            })
    }

    /// Register a handler that can allow or deny navigation attempts.
    pub fn on_navigate(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) -> NavigationPolicy + 'static,
    ) -> Self {
        self.on_navigate = Some(Rc::new(handler));
        self
    }

    /// Register a handler for `window.open` and target-blank requests.
    ///
    /// Without this handler, Kael preserves its compatibility default: if the
    /// normal navigation policy allows the URL, the current WebView navigates to
    /// it and the popup is denied. Use this hook when hosted auth, checkout, or
    /// documentation flows need an explicit popup policy.
    pub fn on_new_window(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) -> WebViewNewWindowPolicy + 'static,
    ) -> Self {
        self.on_new_window = Some(Rc::new(handler));
        self
    }

    /// Deny all `window.open` and target-blank requests.
    pub fn deny_new_windows(self) -> Self {
        self.on_new_window(|_, _, _| WebViewNewWindowPolicy::Deny)
    }

    /// Navigate the current WebView for `window.open` and target-blank requests.
    pub fn open_new_windows_in_current_webview(self) -> Self {
        self.on_new_window(|_, _, _| WebViewNewWindowPolicy::NavigateCurrent)
    }

    /// Let the platform WebView backend handle new-window requests.
    pub fn allow_new_windows(self) -> Self {
        self.on_new_window(|_, _, _| WebViewNewWindowPolicy::Allow)
    }

    /// Register a handler that can allow, deny, or redirect downloads.
    pub fn on_download_started(
        mut self,
        handler: impl Fn(SharedString, Option<PathBuf>, &mut Window, &mut App) -> WebViewDownloadPolicy
        + 'static,
    ) -> Self {
        self.on_download_started = Some(Rc::new(handler));
        self
    }

    /// Register a handler fired when a WebView download completes or fails.
    pub fn on_download_completed(
        mut self,
        handler: impl Fn(WebViewDownloadCompleted, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_download_completed = Some(Rc::new(handler));
        self
    }

    /// Deny all WebView downloads.
    pub fn deny_downloads(self) -> Self {
        self.on_download_started(|_, _, _, _| WebViewDownloadPolicy::Deny)
    }

    /// Allow WebView downloads to use the backend's default destination.
    pub fn allow_downloads(self) -> Self {
        self.on_download_started(|_, _, _, _| WebViewDownloadPolicy::Allow)
    }

    /// Register a handler fired when the WebView document title changes.
    pub fn on_document_title_changed(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_document_title_changed = Some(Rc::new(handler));
        self
    }

    /// Register a handler fired when the WebView starts or finishes loading a page.
    pub fn on_page_load(
        mut self,
        handler: impl Fn(WebViewPageLoadEvent, SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_page_load = Some(Rc::new(handler));
        self
    }

    /// Register a handler for file drag/drop events inside the WebView.
    ///
    /// Return [`WebViewDragDropPolicy::BlockBrowserDefault`] to prevent the
    /// embedded browser from handling the event, or
    /// [`WebViewDragDropPolicy::AllowBrowserDefault`] to preserve browser
    /// behavior such as dropping files onto `<input type="file">`.
    pub fn on_drag_drop(
        mut self,
        handler: impl Fn(WebViewDragDropEvent, &mut Window, &mut App) -> WebViewDragDropPolicy + 'static,
    ) -> Self {
        self.on_drag_drop = Some(Rc::new(handler));
        self
    }

    /// Block browser/OS default handling for file drops inside the WebView.
    pub fn block_drag_drop(self) -> Self {
        self.on_drag_drop(|_, _, _| WebViewDragDropPolicy::BlockBrowserDefault)
    }

    /// Allow navigation only to the supplied URL schemes.
    ///
    /// This is intentionally simple: use [`WebViewOptions::on_navigate`] when
    /// hostnames, paths, OAuth redirect URLs, or app-specific policies matter.
    pub fn allow_navigation_schemes<I, S>(self, schemes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SharedString>,
    {
        let schemes = schemes
            .into_iter()
            .map(|scheme| scheme.into().to_string().to_ascii_lowercase())
            .collect::<Vec<_>>();
        self.on_navigate(move |url, _, _| {
            if url_scheme_is_allowed(url.as_ref(), &schemes) {
                NavigationPolicy::Allow
            } else {
                NavigationPolicy::Deny
            }
        })
    }

    /// Return whether these options use persistent storage.
    pub fn has_persistent_storage(&self) -> bool {
        self.storage_key.is_some()
    }

    /// Return whether developer tools are requested for this WebView.
    pub fn devtools_requested(&self) -> bool {
        self.devtools
    }

    /// Return whether browser zoom hotkeys/gestures are requested.
    pub fn zoom_hotkeys_requested(&self) -> bool {
        self.zoom_hotkeys_enabled
    }

    /// Return an explicit media-autoplay override, if one was requested.
    pub fn media_autoplay_requested(&self) -> Option<bool> {
        self.media_autoplay
    }

    /// Return an explicit initial-focus override, if one was requested.
    pub fn focused_requested(&self) -> Option<bool> {
        self.focused
    }

    /// Return whether JavaScript clipboard access is requested.
    pub fn clipboard_access_requested(&self) -> bool {
        self.clipboard_access
    }

    /// Return request headers configured for the WebView's initial URL load.
    pub fn request_headers_requested(&self) -> Option<&http_client::http::HeaderMap> {
        self.request_headers.as_ref()
    }

    /// Return HTML configured for the WebView's initial document.
    pub fn html_requested(&self) -> Option<&SharedString> {
        self.html.as_ref()
    }

    /// Return whether JavaScript execution is disabled for this WebView.
    pub fn javascript_disabled_requested(&self) -> bool {
        self.javascript_disabled
    }

    /// Return an explicit general-autofill override, if one was requested.
    pub fn general_autofill_requested(&self) -> Option<bool> {
        self.general_autofill
    }

    /// Return the requested native WebView background color, if one was configured.
    pub fn background_color_requested(&self) -> Option<Rgba> {
        self.background_color
    }

    /// Return whether a WebView drag/drop handler is configured.
    pub fn drag_drop_handler_requested(&self) -> bool {
        self.on_drag_drop.is_some()
    }
}

/// A native embedded WebView element.
pub struct WebView {
    element_id: ElementId,
    url: SharedString,
    style: StyleRefinement,
    options: WebViewOptions,
}

impl WebView {
    /// Applies a reusable option bundle to this WebView.
    pub fn options(mut self, options: WebViewOptions) -> Self {
        self.options = options;
        self
    }

    /// Uses persistent storage when a key is supplied and ephemeral storage otherwise.
    pub fn storage_key(mut self, key: impl Into<SharedString>) -> Self {
        self.options.storage_key = Some(key.into());
        self
    }

    /// Overrides the user agent used by the platform WebView.
    pub fn user_agent(mut self, user_agent: impl Into<SharedString>) -> Self {
        self.options.user_agent = Some(user_agent.into());
        self
    }

    /// Injects CSS into each loaded page.
    pub fn inject_css(mut self, css: impl Into<SharedString>) -> Self {
        self.options.injected_css.push(css.into());
        self
    }

    /// Injects JavaScript into each loaded page.
    pub fn inject_javascript(mut self, javascript: impl Into<SharedString>) -> Self {
        self.options.injected_javascript.push(javascript.into());
        self
    }

    /// Loads an HTML string as the WebView's initial document when no URL is supplied.
    pub fn html(mut self, html: impl Into<SharedString>) -> Self {
        self.options = self.options.html(html);
        self
    }

    /// Clears the initial HTML document configured for this WebView.
    pub fn clear_html(mut self) -> Self {
        self.options = self.options.clear_html();
        self
    }

    /// Enables WebView developer tools for development.
    pub fn devtools(mut self) -> Self {
        self.options = self.options.devtools();
        self
    }

    /// Enables or disables WebView developer tools.
    pub fn devtools_enabled(mut self, enabled: bool) -> Self {
        self.options = self.options.devtools_enabled(enabled);
        self
    }

    /// Enables browser zoom hotkeys and gestures where the backend supports them.
    pub fn zoom_hotkeys(mut self) -> Self {
        self.options = self.options.zoom_hotkeys();
        self
    }

    /// Enables or disables browser zoom hotkeys and gestures.
    pub fn zoom_hotkeys_enabled(mut self, enabled: bool) -> Self {
        self.options = self.options.zoom_hotkeys_enabled(enabled);
        self
    }

    /// Allows all WebView media to autoplay without user interaction.
    pub fn media_autoplay(mut self) -> Self {
        self.options = self.options.media_autoplay();
        self
    }

    /// Enables or disables WebView media autoplay without user interaction.
    pub fn media_autoplay_enabled(mut self, enabled: bool) -> Self {
        self.options = self.options.media_autoplay_enabled(enabled);
        self
    }

    /// Requests that this WebView receive focus when created.
    pub fn focused(mut self) -> Self {
        self.options = self.options.focused();
        self
    }

    /// Enables or disables initial WebView focus when the backend supports it.
    pub fn focused_enabled(mut self, focused: bool) -> Self {
        self.options = self.options.focused_enabled(focused);
        self
    }

    /// Enables JavaScript clipboard access for pages that use browser clipboard APIs.
    pub fn clipboard_access(mut self) -> Self {
        self.options = self.options.clipboard_access();
        self
    }

    /// Enables or disables JavaScript clipboard access where the backend supports it.
    pub fn clipboard_access_enabled(mut self, enabled: bool) -> Self {
        self.options = self.options.clipboard_access_enabled(enabled);
        self
    }

    /// Adds request headers for the WebView's initial URL load.
    pub fn request_headers(mut self, headers: http_client::http::HeaderMap) -> Self {
        self.options = self.options.request_headers(headers);
        self
    }

    /// Removes request headers from the WebView's initial URL load.
    pub fn clear_request_headers(mut self) -> Self {
        self.options = self.options.clear_request_headers();
        self
    }

    /// Disables JavaScript for untrusted or static browser content.
    pub fn javascript_disabled(mut self) -> Self {
        self.options = self.options.javascript_disabled();
        self
    }

    /// Enables or disables JavaScript execution where the backend supports it.
    pub fn javascript_disabled_enabled(mut self, disabled: bool) -> Self {
        self.options = self.options.javascript_disabled_enabled(disabled);
        self
    }

    /// Disables browser-level general autofill where the backend supports it.
    pub fn general_autofill_disabled(mut self) -> Self {
        self.options = self.options.general_autofill_disabled();
        self
    }

    /// Enables or disables browser-level general autofill where the backend supports it.
    pub fn general_autofill_enabled(mut self, enabled: bool) -> Self {
        self.options = self.options.general_autofill_enabled(enabled);
        self
    }

    /// Sets the native WebView background color.
    pub fn background_color(mut self, color: impl Into<Rgba>) -> Self {
        self.options = self.options.background_color(color);
        self
    }

    /// Requests a transparent native WebView background.
    pub fn transparent_background(mut self) -> Self {
        self.options = self.options.transparent_background();
        self
    }

    /// Registers a handler for messages posted from JavaScript.
    pub fn on_message(
        mut self,
        handler: impl Fn(serde_json::Value, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options.on_message = Some(Rc::new(handler));
        self
    }

    /// Registers a handler for typed WebView bridge envelopes.
    pub fn on_bridge_message(
        mut self,
        handler: impl Fn(WebViewBridgeMessage, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_bridge_message(handler);
        self
    }

    /// Injects the standard `window.kael` bridge helper.
    pub fn bridge_script(mut self) -> Self {
        self.options = self.options.bridge_script();
        self
    }

    /// Injects same-document navigation state tracking for app-owned pages.
    pub fn navigation_state_bridge(mut self) -> Self {
        self.options = self.options.navigation_state_bridge();
        self
    }

    /// Injects the standard bridge plus native desktop find-result forwarding.
    pub fn find_result_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.find_result_bridge(kind);
        self
    }

    /// Injects find-result forwarding and handles typed find events.
    pub fn on_find_result(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFindEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_find_result(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser media event forwarding.
    pub fn media_event_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.media_event_bridge(kind);
        self
    }

    /// Injects browser media event forwarding and handles typed media events.
    pub fn on_media_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewMediaEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_media_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser context-menu forwarding.
    pub fn context_menu_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.context_menu_bridge(kind);
        self
    }

    /// Injects browser context-menu forwarding and handles typed context events.
    pub fn on_context_menu(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewContextMenuEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_context_menu(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser pointer context forwarding.
    pub fn pointer_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.pointer_bridge(kind);
        self
    }

    /// Injects browser pointer context forwarding and handles typed pointer events.
    pub fn on_pointer_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewPointerEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_pointer_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser form activity forwarding.
    pub fn form_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.form_bridge(kind);
        self
    }

    /// Injects browser form forwarding and handles typed form events.
    pub fn on_form_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFormEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_form_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser file-input selection forwarding.
    pub fn file_input_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.file_input_bridge(kind);
        self
    }

    /// Injects file-input forwarding and handles typed file-input events.
    pub fn on_file_input_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFileInputEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_file_input_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser resource observability.
    pub fn resource_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.resource_bridge(kind);
        self
    }

    /// Injects resource forwarding and handles typed resource events.
    pub fn on_resource_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewResourceEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_resource_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus hosted fetch/XHR network observability.
    pub fn network_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.network_bridge(kind);
        self
    }

    /// Injects fetch/XHR forwarding and handles typed network events.
    pub fn on_network_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewNetworkEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_network_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser dialog observability.
    pub fn dialog_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.dialog_bridge(kind);
        self
    }

    /// Injects browser dialog forwarding and handles typed dialog events.
    pub fn on_dialog_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewDialogEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_dialog_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser clipboard event forwarding.
    pub fn clipboard_event_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.clipboard_event_bridge(kind);
        self
    }

    /// Injects clipboard event forwarding and handles typed clipboard events.
    pub fn on_clipboard_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewClipboardEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_clipboard_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser permission preflighting.
    pub fn permission_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.permission_bridge(kind);
        self
    }

    /// Injects permission preflighting and handles typed permission requests.
    pub fn on_permission_request(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewPermissionRequest, &mut Window, &mut App) -> WebViewPermissionDecision
        + 'static,
    ) -> Self {
        self.options = self.options.on_permission_request(kind, handler);
        self
    }

    /// Injects the standard bridge plus Web Storage event forwarding.
    pub fn storage_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.storage_bridge(kind);
        self
    }

    /// Injects Web Storage forwarding and handles typed storage events.
    pub fn on_storage_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewStorageEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_storage_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser favicon change forwarding.
    pub fn favicon_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.favicon_bridge(kind);
        self
    }

    /// Injects browser favicon forwarding and handles typed favicon events.
    pub fn on_favicon_changed(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewFaviconEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_favicon_changed(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser console forwarding.
    pub fn console_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.console_bridge(kind);
        self
    }

    /// Injects browser console forwarding and handles typed console events.
    pub fn on_console_message(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewConsoleEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_console_message(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser keyboard/input forwarding.
    pub fn keyboard_event_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.keyboard_event_bridge(kind);
        self
    }

    /// Injects browser keyboard/input forwarding and handles typed events.
    pub fn on_keyboard_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewKeyboardEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_keyboard_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus same-document location forwarding.
    pub fn location_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.location_bridge(kind);
        self
    }

    /// Injects location forwarding and handles typed location events.
    pub fn on_location_changed(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewLocationEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_location_changed(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser lifecycle/focus forwarding.
    pub fn lifecycle_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.lifecycle_bridge(kind);
        self
    }

    /// Injects lifecycle/focus forwarding and handles typed lifecycle events.
    pub fn on_lifecycle_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewLifecycleEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_lifecycle_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser scroll/viewport forwarding.
    pub fn scroll_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.scroll_bridge(kind);
        self
    }

    /// Injects scroll/viewport forwarding and handles typed scroll events.
    pub fn on_scroll_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewScrollEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_scroll_event(kind, handler);
        self
    }

    /// Injects the standard bridge plus browser selection forwarding.
    pub fn selection_bridge(mut self, kind: impl AsRef<str>) -> Self {
        self.options = self.options.selection_bridge(kind);
        self
    }

    /// Injects selection forwarding and handles typed selection events.
    pub fn on_selection_event(
        mut self,
        kind: impl Into<SharedString>,
        handler: impl Fn(WebViewSelectionEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_selection_event(kind, handler);
        self
    }

    /// Registers a handler that can allow or deny navigation attempts.
    pub fn on_navigate(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) -> NavigationPolicy + 'static,
    ) -> Self {
        self.options.on_navigate = Some(Rc::new(handler));
        self
    }

    /// Registers a handler for `window.open` and target-blank requests.
    pub fn on_new_window(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) -> WebViewNewWindowPolicy + 'static,
    ) -> Self {
        self.options = self.options.on_new_window(handler);
        self
    }

    /// Denies all `window.open` and target-blank requests.
    pub fn deny_new_windows(mut self) -> Self {
        self.options = self.options.deny_new_windows();
        self
    }

    /// Navigates the current WebView for `window.open` and target-blank requests.
    pub fn open_new_windows_in_current_webview(mut self) -> Self {
        self.options = self.options.open_new_windows_in_current_webview();
        self
    }

    /// Lets the platform WebView backend handle new-window requests.
    pub fn allow_new_windows(mut self) -> Self {
        self.options = self.options.allow_new_windows();
        self
    }

    /// Registers a handler that can allow, deny, or redirect downloads.
    pub fn on_download_started(
        mut self,
        handler: impl Fn(SharedString, Option<PathBuf>, &mut Window, &mut App) -> WebViewDownloadPolicy
        + 'static,
    ) -> Self {
        self.options = self.options.on_download_started(handler);
        self
    }

    /// Registers a handler fired when a WebView download completes or fails.
    pub fn on_download_completed(
        mut self,
        handler: impl Fn(WebViewDownloadCompleted, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_download_completed(handler);
        self
    }

    /// Denies all WebView downloads.
    pub fn deny_downloads(mut self) -> Self {
        self.options = self.options.deny_downloads();
        self
    }

    /// Allows WebView downloads to use the backend's default destination.
    pub fn allow_downloads(mut self) -> Self {
        self.options = self.options.allow_downloads();
        self
    }

    /// Registers a handler fired when the WebView document title changes.
    pub fn on_document_title_changed(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_document_title_changed(handler);
        self
    }

    /// Registers a handler fired when the WebView starts or finishes loading a page.
    pub fn on_page_load(
        mut self,
        handler: impl Fn(WebViewPageLoadEvent, SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options = self.options.on_page_load(handler);
        self
    }

    /// Registers a handler for file drag/drop events inside the WebView.
    pub fn on_drag_drop(
        mut self,
        handler: impl Fn(WebViewDragDropEvent, &mut Window, &mut App) -> WebViewDragDropPolicy + 'static,
    ) -> Self {
        self.options = self.options.on_drag_drop(handler);
        self
    }

    /// Blocks browser/OS default handling for file drops inside the WebView.
    pub fn block_drag_drop(mut self) -> Self {
        self.options = self.options.block_drag_drop();
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
        let message_handler = if let Some((permission_kind, permission_handler)) =
            self.options.on_permission_request.clone()
        {
            let previous = self.options.on_message.clone();
            let controller = WebViewController::new(self.element_id.clone());
            Some(Rc::new(
                move |value: serde_json::Value, window: &mut Window, cx: &mut App| {
                    if let Some(previous) = previous.as_ref() {
                        previous(value.clone(), window, cx);
                    }
                    if let Some(message) = WebViewBridgeMessage::from_value(value) {
                        if let Some(request) = WebViewPermissionRequest::from_bridge_message(
                            &message,
                            permission_kind.as_ref(),
                        ) {
                            let decision = permission_handler(request, window, cx);
                            let _ = controller.respond_to_bridge_message(
                                window,
                                &message,
                                serde_json::json!({ "decision": decision.as_bridge_str() }),
                            );
                        }
                    }
                },
            ) as WebViewMessageHandler)
        } else {
            self.options.on_message.clone()
        };

        let webview = PlatformWebView {
            id: self.element_id.to_string().into(),
            bounds,
            url: self.url.clone(),
            html: self.options.html.clone(),
            visible: true,
            storage_key: self.options.storage_key.clone(),
            user_agent: self.options.user_agent.clone(),
            injected_css: self.options.injected_css.clone(),
            injected_javascript: self.options.injected_javascript.clone(),
            request_headers: self.options.request_headers.clone(),
            javascript_disabled: self.options.javascript_disabled,
            general_autofill: self.options.general_autofill,
            background_color: self.options.background_color,
            devtools: self.options.devtools,
            zoom_hotkeys_enabled: self.options.zoom_hotkeys_enabled,
            media_autoplay: self.options.media_autoplay,
            focused: self.options.focused,
            clipboard_access: self.options.clipboard_access,
            async_window: window.to_async(cx),
            message_handler,
            navigation_handler: self.options.on_navigate.clone(),
            new_window_handler: self.options.on_new_window.clone(),
            download_started_handler: self.options.on_download_started.clone(),
            download_completed_handler: self.options.on_download_completed.clone(),
            document_title_changed_handler: self.options.on_document_title_changed.clone(),
            page_load_handler: self.options.on_page_load.clone(),
            drag_drop_handler: self.options.on_drag_drop.clone(),
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

fn url_scheme_is_allowed(url: &str, allowed: &[String]) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let scheme = scheme.to_ascii_lowercase();
    allowed.iter().any(|allowed| allowed == &scheme)
}

#[cfg(test)]
mod tests {
    use super::{
        WebViewBridgeMessage, WebViewClipboardEvent, WebViewConsoleEvent, WebViewContextMenuEvent,
        WebViewController, WebViewCookie, WebViewDialogEvent, WebViewDocumentSnapshot,
        WebViewDomImageCaptureOptions, WebViewDownloadCompleted, WebViewDownloadPolicy,
        WebViewDownloadTriggerResult, WebViewDragDropEvent, WebViewDragDropPolicy,
        WebViewEditCommand, WebViewElementSnapshot, WebViewFaviconEvent, WebViewFileInputEvent,
        WebViewFindEvent, WebViewFindOptions, WebViewFindResult, WebViewFormEvent,
        WebViewKeyboardEvent, WebViewLifecycleEvent, WebViewLocationEvent, WebViewMediaCommand,
        WebViewMediaElementOptions, WebViewMediaElementState, WebViewMediaEvent,
        WebViewMediaFrameCaptureOptions, WebViewMediaTextTrackOptions, WebViewNetworkEvent,
        WebViewNewWindowPolicy, WebViewOptions, WebViewPageLoadEvent, WebViewPermissionDecision,
        WebViewPermissionRequest, WebViewPointerEvent, WebViewResourceEvent, WebViewScrollEvent,
        WebViewSelectionEvent, WebViewStopFindAction, WebViewStorageArea, WebViewStorageEvent,
        WebViewStorageMutationResult, WebViewStorageSnapshot, parse_webview_bool_result,
        parse_webview_document_snapshot_result, parse_webview_download_trigger_result,
        parse_webview_element_snapshot_result, parse_webview_find_result,
        parse_webview_media_state_result, parse_webview_optional_scroll_event_result,
        parse_webview_optional_string_result, parse_webview_scroll_event_result,
        parse_webview_storage_mutation_result, parse_webview_storage_snapshot_result,
        parse_webview_string_array_result, parse_webview_string_result, url_scheme_is_allowed,
        webview, webview_add_media_text_track_script, webview_attribute_action_script,
        webview_bridge_script, webview_can_go_back_script, webview_can_go_forward_script,
        webview_capture_dom_image_script, webview_capture_media_frame_script,
        webview_class_action_script, webview_clear_storage_area_script,
        webview_click_selector_script, webview_clipboard_event_bridge_script,
        webview_console_bridge_script, webview_context_menu_bridge_script,
        webview_dialog_bridge_script, webview_disable_media_text_tracks_script,
        webview_document_html_script, webview_document_snapshot_script,
        webview_edit_command_script, webview_element_snapshot_script,
        webview_exit_media_fullscreen_script, webview_exit_media_picture_in_picture_script,
        webview_favicon_bridge_script, webview_favicons_script, webview_file_input_bridge_script,
        webview_file_url, webview_file_with_options, webview_find_result_bridge_script,
        webview_find_result_script, webview_find_script, webview_focus_selector_script,
        webview_form_bridge_script, webview_html_url, webview_html_with_options,
        webview_insert_css_script, webview_insert_text_script, webview_is_loading_script,
        webview_keyboard_bridge_script, webview_lifecycle_bridge_script,
        webview_location_bridge_script, webview_media_command_script,
        webview_media_event_bridge_script, webview_media_state_script,
        webview_navigation_state_bridge_script, webview_network_bridge_script,
        webview_pause_media_script, webview_permission_bridge_script, webview_play_media_script,
        webview_pointer_bridge_script, webview_remove_inserted_css_script,
        webview_remove_media_text_track_script, webview_remove_storage_item_script,
        webview_request_media_fullscreen_script, webview_request_media_picture_in_picture_script,
        webview_reset_form_script, webview_resource_bridge_script, webview_scroll_bridge_script,
        webview_scroll_by_script, webview_scroll_selector_into_view_script,
        webview_scroll_to_script, webview_seek_media_secs_script,
        webview_select_media_text_track_script, webview_selected_html_script,
        webview_selected_text_script, webview_selection_bridge_script,
        webview_set_form_value_script, webview_set_media_muted_script,
        webview_set_media_options_script, webview_set_media_playback_rate_script,
        webview_set_media_source_script, webview_set_media_volume_script,
        webview_set_storage_item_script, webview_stop_finding_script,
        webview_storage_bridge_script, webview_storage_snapshot_script,
        webview_style_property_script, webview_submit_form_script, webview_trigger_download_script,
        webview_viewport_snapshot_script, webview_with_options,
    };
    use crate::{SharedString, rgb};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn webview_controller_preserves_target_id() {
        let controller = WebViewController::new("docs-webview");
        assert_eq!(controller.id().as_ref(), "docs-webview");
    }

    #[test]
    fn webview_controller_exposes_browser_surface_commands() {
        let controller = WebViewController::new("docs-webview");
        assert_eq!(controller.id().as_ref(), "docs-webview");
        let cloned = controller.clone();
        assert_eq!(cloned, controller);
        let _navigate_with_headers = WebViewController::navigate_with_headers
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                http_client::http::HeaderMap,
            ) -> anyhow::Result<()>;
        let _load_html = WebViewController::load_html
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
            ) -> anyhow::Result<()>;
        let _evaluate_javascript_with_result = WebViewController::evaluate_javascript_with_result
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _insert_css = WebViewController::insert_css
            as fn(&WebViewController, &mut crate::Window, &str, &str) -> anyhow::Result<()>;
        let _remove_inserted_css = WebViewController::remove_inserted_css
            as fn(&WebViewController, &mut crate::Window, &str) -> anyhow::Result<()>;
        let _find_text = WebViewController::find_text
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewFindOptions,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _find_text_result = WebViewController::find_text_result
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewFindOptions,
                fn(Result<WebViewFindResult, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _stop_finding = WebViewController::stop_finding
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _stop_finding_with_action = WebViewController::stop_finding_with_action
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewStopFindAction,
            ) -> anyhow::Result<()>;
        let _edit_command = WebViewController::edit_command
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewEditCommand,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _insert_text = WebViewController::insert_text
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _focus_selector = WebViewController::focus_selector
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _click_selector = WebViewController::click_selector
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _add_class = WebViewController::add_class
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _remove_class = WebViewController::remove_class
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _toggle_class = WebViewController::toggle_class
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                Option<bool>,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_attribute = WebViewController::set_attribute
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _remove_attribute = WebViewController::remove_attribute
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_style_property = WebViewController::set_style_property
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _remove_style_property = WebViewController::remove_style_property
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_form_value = WebViewController::set_form_value
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _submit_form = WebViewController::submit_form
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _reset_form = WebViewController::reset_form
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _copy = WebViewController::copy
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _cut = WebViewController::cut
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _paste = WebViewController::paste
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _select_all = WebViewController::select_all
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _undo = WebViewController::undo
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _redo = WebViewController::redo
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _delete_selection = WebViewController::delete_selection
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _selected_text = WebViewController::selected_text
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _selected_html = WebViewController::selected_html
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _document_html = WebViewController::document_html
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _document_snapshot = WebViewController::document_snapshot
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<WebViewDocumentSnapshot, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _element_snapshot = WebViewController::element_snapshot
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<Option<WebViewElementSnapshot>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _capture_dom_image = WebViewController::capture_dom_image
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewDomImageCaptureOptions,
                fn(Result<Option<crate::SharedString>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _trigger_download = WebViewController::trigger_download
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                Option<crate::SharedString>,
                fn(Result<WebViewDownloadTriggerResult, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _download_url = WebViewController::download_url
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                Option<crate::SharedString>,
                fn(Result<WebViewDownloadTriggerResult, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _favicons = WebViewController::favicons
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<Vec<crate::SharedString>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _title = WebViewController::title
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _user_agent = WebViewController::user_agent
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _is_loading = WebViewController::is_loading
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _can_go_back = WebViewController::can_go_back
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _can_go_forward = WebViewController::can_go_forward
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _viewport_snapshot = WebViewController::viewport_snapshot
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<WebViewScrollEvent, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _scroll_to = WebViewController::scroll_to
            as fn(
                &WebViewController,
                &mut crate::Window,
                f64,
                f64,
                fn(Result<WebViewScrollEvent, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _scroll_by = WebViewController::scroll_by
            as fn(
                &WebViewController,
                &mut crate::Window,
                f64,
                f64,
                fn(Result<WebViewScrollEvent, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _scroll_selector_into_view = WebViewController::scroll_selector_into_view
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<Option<WebViewScrollEvent>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _stop_loading = WebViewController::stop_loading
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _play_media = WebViewController::play_media
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _pause_media = WebViewController::pause_media
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _set_media_muted = WebViewController::set_media_muted
            as fn(&WebViewController, &mut crate::Window, bool) -> anyhow::Result<()>;
        let _set_media_volume = WebViewController::set_media_volume
            as fn(&WebViewController, &mut crate::Window, f32) -> anyhow::Result<()>;
        let _set_media_playback_rate = WebViewController::set_media_playback_rate
            as fn(&WebViewController, &mut crate::Window, f32) -> anyhow::Result<()>;
        let _seek_media_secs = WebViewController::seek_media_secs
            as fn(&WebViewController, &mut crate::Window, f64) -> anyhow::Result<()>;
        let _media_command = WebViewController::media_command
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewMediaCommand,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_media_source = WebViewController::set_media_source
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_media_options = WebViewController::set_media_options
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewMediaElementOptions,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _capture_media_frame = WebViewController::capture_media_frame
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewMediaFrameCaptureOptions,
                fn(Result<Option<crate::SharedString>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _add_media_text_track = WebViewController::add_media_text_track
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                WebViewMediaTextTrackOptions,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _remove_media_text_track = WebViewController::remove_media_text_track
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                crate::SharedString,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _select_media_text_track = WebViewController::select_media_text_track
            as fn(&WebViewController, &mut crate::Window, &str) -> anyhow::Result<()>;
        let _disable_media_text_tracks = WebViewController::disable_media_text_tracks
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _request_media_fullscreen = WebViewController::request_media_fullscreen
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _exit_media_fullscreen = WebViewController::exit_media_fullscreen
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _request_media_picture_in_picture = WebViewController::request_media_picture_in_picture
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _exit_media_picture_in_picture = WebViewController::exit_media_picture_in_picture
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _media_state = WebViewController::media_state
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<Vec<WebViewMediaElementState>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _mute_media = WebViewController::mute_media
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _unmute_media = WebViewController::unmute_media
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _clear_browsing_data = WebViewController::clear_browsing_data
            as fn(&WebViewController, &mut crate::Window) -> anyhow::Result<()>;
        let _storage_snapshot = WebViewController::storage_snapshot
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<WebViewStorageSnapshot, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_storage_item = WebViewController::set_storage_item
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewStorageArea,
                crate::SharedString,
                crate::SharedString,
                fn(Result<WebViewStorageMutationResult, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _remove_storage_item = WebViewController::remove_storage_item
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewStorageArea,
                crate::SharedString,
                fn(Result<WebViewStorageMutationResult, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _clear_storage_area = WebViewController::clear_storage_area
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewStorageArea,
                fn(Result<WebViewStorageMutationResult, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _is_devtools_open = WebViewController::is_devtools_open
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<bool, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _url = WebViewController::url
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<crate::SharedString, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _cookies = WebViewController::cookies
            as fn(
                &WebViewController,
                &mut crate::Window,
                fn(Result<Vec<WebViewCookie>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _cookies_for_url = WebViewController::cookies_for_url
            as fn(
                &WebViewController,
                &mut crate::Window,
                crate::SharedString,
                fn(Result<Vec<WebViewCookie>, crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _set_cookie = WebViewController::set_cookie
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewCookie,
                fn(Result<(), crate::SharedString>),
            ) -> anyhow::Result<()>;
        let _delete_cookie = WebViewController::delete_cookie
            as fn(
                &WebViewController,
                &mut crate::Window,
                WebViewCookie,
                fn(Result<(), crate::SharedString>),
            ) -> anyhow::Result<()>;
    }

    #[test]
    fn webview_cookie_shape_preserves_browser_cookie_metadata() {
        let cookie = WebViewCookie {
            name: "session".into(),
            value: "abc".into(),
            domain: Some("example.com".into()),
            path: Some("/account".into()),
            secure: true,
            http_only: true,
        };

        assert_eq!(cookie.name.as_ref(), "session");
        assert_eq!(cookie.value.as_ref(), "abc");
        assert_eq!(
            cookie.domain.as_ref().map(|value| value.as_ref()),
            Some("example.com")
        );
        assert_eq!(
            cookie.path.as_ref().map(|value| value.as_ref()),
            Some("/account")
        );
        assert!(cookie.secure);
        assert!(cookie.http_only);

        let built = WebViewCookie::new("preview", "enabled")
            .domain("example.com")
            .path("/")
            .secure(true)
            .http_only(true);
        assert_eq!(built.name.as_ref(), "preview");
        assert_eq!(built.value.as_ref(), "enabled");
        assert_eq!(
            built.domain.as_ref().map(|value| value.as_ref()),
            Some("example.com")
        );
        assert_eq!(built.path.as_ref().map(|value| value.as_ref()), Some("/"));
        assert!(built.secure);
        assert!(built.http_only);
    }

    #[test]
    fn webview_options_presets_capture_common_island_policies() {
        let auth = WebViewOptions::auth_flow("my-app.auth");
        assert!(auth.has_persistent_storage());
        assert_eq!(auth.injected_css.len(), 1);
        assert!(auth.on_navigate.is_some());

        let graphics = WebViewOptions::web_graphics();
        assert!(!graphics.has_persistent_storage());
        assert_eq!(graphics.injected_css.len(), 2);
        assert!(graphics.on_navigate.is_some());
        assert!(!graphics.devtools_requested());
    }

    #[test]
    fn webview_options_can_request_devtools() {
        let options = WebViewOptions::embedded_widget().devtools();
        assert!(options.devtools_requested());

        let element = webview_with_options(
            "debug-widget",
            "https://example.com",
            WebViewOptions::embedded_widget().devtools_enabled(true),
        );
        assert!(element.options.devtools_requested());
    }

    #[test]
    fn webview_options_can_request_browser_zoom_hotkeys() {
        let options = WebViewOptions::embedded_widget().zoom_hotkeys();
        assert!(options.zoom_hotkeys_requested());

        let element = webview_with_options(
            "zoomable-docs",
            "https://example.com",
            WebViewOptions::embedded_widget().zoom_hotkeys_enabled(true),
        );
        assert!(element.options.zoom_hotkeys_requested());

        let disabled = element.zoom_hotkeys_enabled(false);
        assert!(!disabled.options.zoom_hotkeys_requested());
    }

    #[test]
    fn webview_find_options_generate_window_find_script() {
        let options = WebViewFindOptions::backward()
            .case_sensitive(true)
            .whole_word(true);
        let script = webview_find_script("Needle \"quoted\"", options);

        assert!(script.contains("window.find"));
        assert!(script.contains("\"Needle \\\"quoted\\\"\""));
        assert!(script.contains("true, true, true, true, true, false"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
        assert!(parse_webview_bool_result("\"maybe\"").is_err());
    }

    #[test]
    fn webview_find_result_script_counts_document_matches() {
        let options = WebViewFindOptions::forward().whole_word(true);
        let script = webview_find_result_script("Needle \"quoted\"", options);
        let result = parse_webview_find_result(r#"{"found":true,"matches":3}"#).unwrap();

        assert!(script.contains("window.find"));
        assert!(script.contains("createTreeWalker"));
        assert!(script.contains("NodeFilter.SHOW_TEXT"));
        assert!(script.contains("matches"));
        assert!(script.contains("wholeWord"));
        assert!(script.contains("\"Needle \\\"quoted\\\"\""));
        assert!(result.found);
        assert_eq!(result.matches, 3);
        assert!(parse_webview_find_result("true").is_err());
    }

    #[test]
    fn webview_find_result_bridge_script_forwards_found_in_page_events() {
        let script = webview_find_result_bridge_script("find:result \"quoted\"");

        assert!(script.contains("\"find:result \\\"quoted\\\"\""));
        assert!(script.contains("const originalFind"));
        assert!(script.contains("window.find = function(query"));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("event: \"find\""));
        assert!(script.contains("matches: countMatches"));
        assert!(script.contains("selectionText"));
        assert!(script.contains("caseSensitive: !!caseSensitive"));
        assert!(script.contains("searchInFrames: !!searchInFrames"));
        assert!(script.contains("__kaelFindResultBridge"));
    }

    #[test]
    fn webview_stop_finding_script_handles_browser_runtime_actions() {
        let clear = webview_stop_finding_script(WebViewStopFindAction::ClearSelection);
        let keep = webview_stop_finding_script(WebViewStopFindAction::KeepSelection);
        let activate = webview_stop_finding_script(WebViewStopFindAction::ActivateSelection);

        assert!(clear.contains("clearSelection"));
        assert!(clear.contains("removeAllRanges"));
        assert!(clear.contains("document.activeElement.blur"));
        assert!(keep.contains("const action = \"keepSelection\""));
        assert!(activate.contains("activateSelection"));
        assert!(activate.contains("scrollIntoView"));
        assert!(activate.contains("target.focus"));
    }

    #[test]
    fn webview_string_result_parser_reads_json_strings() {
        assert_eq!(
            parse_webview_string_result("\"Docs \\\"Preview\\\"\"").unwrap(),
            SharedString::from("Docs \"Preview\"")
        );
        assert_eq!(
            parse_webview_string_result("null").unwrap(),
            SharedString::default()
        );
        assert!(parse_webview_string_result("42").is_err());
    }

    #[test]
    fn webview_optional_string_result_parser_reads_json_strings_and_null() {
        assert_eq!(
            parse_webview_optional_string_result("\"data:image/png;base64,abc\"").unwrap(),
            Some(SharedString::from("data:image/png;base64,abc"))
        );
        assert_eq!(parse_webview_optional_string_result("null").unwrap(), None);
        assert!(parse_webview_optional_string_result("42").is_err());
    }

    #[test]
    fn webview_is_loading_script_reads_document_ready_state() {
        let script = webview_is_loading_script();

        assert!(script.contains("document.readyState"));
        assert!(script.contains("\"complete\""));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_can_go_back_script_reads_history_length() {
        let script = webview_can_go_back_script();

        assert!(script.contains("history.length"));
        assert!(script.contains("> 1"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_can_go_forward_script_reads_navigation_state_marker() {
        let script = webview_can_go_forward_script();

        assert!(script.contains("__kaelNavigationState"));
        assert!(script.contains("kaelNavigationState"));
        assert!(script.contains("canGoForward"));
        assert!(script.contains("return false"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_navigation_state_bridge_tracks_same_document_history() {
        let script = webview_navigation_state_bridge_script();

        assert!(script.contains("__kaelNavigationState"));
        assert!(script.contains("kaelNavigationState"));
        assert!(script.contains("history.pushState"));
        assert!(script.contains("history.replaceState"));
        assert!(script.contains("popstate"));
        assert!(script.contains("canGoForward: index < max"));
    }

    #[test]
    fn webview_edit_commands_generate_exec_command_script() {
        let copy = webview_edit_command_script(WebViewEditCommand::Copy);
        let select_all = webview_edit_command_script(WebViewEditCommand::SelectAll);
        let delete = webview_edit_command_script(WebViewEditCommand::Delete);

        assert!(copy.contains("document.execCommand"));
        assert!(copy.contains("\"copy\""));
        assert!(select_all.contains("\"selectAll\""));
        assert!(delete.contains("\"delete\""));
    }

    #[test]
    fn webview_insert_text_script_types_into_focused_editors() {
        let script = webview_insert_text_script("Hello \"Kael\"\n");

        assert!(script.contains("document.execCommand(\"insertText\", false, text)"));
        assert!(script.contains("Hello \\\"Kael\\\"\\n"));
        assert!(script.contains("active.setRangeText(text, start, end, \"end\")"));
        assert!(script.contains("active.value.slice(0, start) + text + active.value.slice(end)"));
        assert!(script.contains("active.isContentEditable"));
        assert!(script.contains("range.insertNode(node)"));
        assert!(script.contains("inputType: \"insertText\""));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_selector_action_scripts_focus_and_click_matches() {
        let focus = webview_focus_selector_script("#search \"quoted\"");
        let click = webview_click_selector_script("button.primary");

        assert!(focus.contains("document.querySelector(selector)"));
        assert!(focus.contains("#search \\\"quoted\\\""));
        assert!(focus.contains("element.scrollIntoView"));
        assert!(focus.contains("element.focus({ preventScroll: true })"));
        assert!(focus.contains("element.focus();"));
        assert!(focus.contains("document.activeElement === element"));
        assert!(click.contains("const action = \"click\""));
        assert!(click.contains("element.click();"));
        assert!(click.contains("return false"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_dom_customization_scripts_mutate_selected_elements() {
        let add_class = webview_class_action_script(".card", "is-active", "add", None);
        let toggle_class = webview_class_action_script(".card", "is-open", "toggle", Some(false));
        let set_attr = webview_attribute_action_script(
            "button[data-id=\"42\"]",
            "aria-expanded",
            Some("true"),
        );
        let remove_attr = webview_attribute_action_script(".panel", "hidden", None);
        let set_style = webview_style_property_script(".panel", "--accent-color", Some("#f04"));
        let remove_style = webview_style_property_script(".panel", "display", None);

        assert!(add_class.contains("document.querySelector(selector)"));
        assert!(add_class.contains("element.classList.add(className)"));
        assert!(toggle_class.contains("element.classList.toggle(className, force)"));
        assert!(toggle_class.contains("const force = false"));
        assert!(set_attr.contains("button[data-id=\\\"42\\\"]"));
        assert!(set_attr.contains("element.setAttribute(name, String(value))"));
        assert!(remove_attr.contains("element.removeAttribute(name)"));
        assert!(set_style.contains("element.style.setProperty(name, String(value))"));
        assert!(set_style.contains("--accent-color"));
        assert!(remove_style.contains("element.style.removeProperty(name)"));
        assert!(remove_style.contains("const value = null"));
    }

    #[test]
    fn webview_set_form_value_script_updates_common_controls() {
        let script = webview_set_form_value_script("select[name=\"plan\"]", "pro");

        assert!(script.contains("document.querySelector(selector)"));
        assert!(script.contains("select[name=\\\"plan\\\"]"));
        assert!(script.contains("tag === \"select\""));
        assert!(script.contains("option.value === value || option.text === value"));
        assert!(script.contains("element.multiple"));
        assert!(script.contains("type === \"checkbox\" || type === \"radio\""));
        assert!(script.contains("element.checked = next"));
        assert!(script.contains("typeof element.value === \"string\""));
        assert!(script.contains("element.isContentEditable"));
        assert!(script.contains("emit(element, \"input\")"));
        assert!(script.contains("emit(element, \"change\")"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_submit_form_script_uses_browser_submit_flow() {
        let script = webview_submit_form_script("#checkout \"quoted\"");

        assert!(script.contains("document.querySelector(selector)"));
        assert!(script.contains("#checkout \\\"quoted\\\""));
        assert!(script.contains("target.closest(\"form\")"));
        assert!(script.contains("form.requestSubmit()"));
        assert!(script.contains("cancelable: true"));
        assert!(script.contains("form.dispatchEvent(event)"));
        assert!(script.contains("form.submit()"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_reset_form_script_uses_browser_reset_flow() {
        let script = webview_reset_form_script(".settings \"quoted\"");

        assert!(script.contains("document.querySelector(selector)"));
        assert!(script.contains(".settings \\\"quoted\\\""));
        assert!(script.contains("target.closest(\"form\")"));
        assert!(script.contains("typeof form.reset !== \"function\""));
        assert!(script.contains("form.reset()"));
        assert_eq!(parse_webview_bool_result("true"), Ok(true));
        assert_eq!(parse_webview_bool_result("false"), Ok(false));
    }

    #[test]
    fn webview_selected_text_script_reads_document_and_input_selection() {
        let script = webview_selected_text_script();

        assert!(script.contains("document.activeElement"));
        assert!(script.contains("selectionStart"));
        assert!(script.contains("selectionEnd"));
        assert!(script.contains("active.value.slice"));
        assert!(script.contains("window.getSelection"));
        assert!(script.contains("selection.toString()"));
        assert_eq!(
            parse_webview_string_result("\"Selected text\"").unwrap(),
            SharedString::from("Selected text")
        );
    }

    #[test]
    fn webview_selected_html_script_serializes_document_and_input_selection() {
        let script = webview_selected_html_script();

        assert!(script.contains("document.createElement(\"div\")"));
        assert!(script.contains("document.activeElement"));
        assert!(script.contains("active.value.slice"));
        assert!(script.contains("container.textContent"));
        assert!(script.contains("window.getSelection"));
        assert!(script.contains("selection.rangeCount"));
        assert!(script.contains("cloneContents()"));
        assert!(script.contains("container.innerHTML"));
        assert_eq!(
            parse_webview_string_result("\"<strong>Selected</strong>\"").unwrap(),
            SharedString::from("<strong>Selected</strong>")
        );
    }

    #[test]
    fn webview_document_html_script_serializes_document_element() {
        let script = webview_document_html_script();

        assert!(script.contains("document.documentElement"));
        assert!(script.contains("outerHTML"));
        assert!(script.contains("return \"\""));
        assert_eq!(
            parse_webview_string_result("\"<html><body>Docs</body></html>\"").unwrap(),
            SharedString::from("<html><body>Docs</body></html>")
        );
    }

    #[test]
    fn webview_document_snapshot_script_serializes_document_outline() {
        let script = webview_document_snapshot_script();
        let snapshot = parse_webview_document_snapshot_result(
            r#"{"url":"https://example.test/docs","title":"Docs","readyState":"complete","language":"en","direction":"ltr","visibleText":"Docs Welcome Open settings","textLength":26,"headings":[{"level":1,"text":"Docs","id":"top"}],"links":[{"text":"Open settings","href":"https://example.test/settings","target":"_blank"}],"images":[{"src":"https://example.test/logo.png","alt":"Logo","title":"Brand"}],"forms":[{"id":"login","name":"login","action":"https://example.test/login","method":"post","controlCount":2}]}"#,
        )
        .unwrap();

        assert!(script.contains("document.querySelectorAll(\"h1,h2,h3,h4,h5,h6\")"));
        assert!(script.contains("document.querySelectorAll(\"a[href]\")"));
        assert!(script.contains("document.querySelectorAll(\"img[src]\")"));
        assert!(script.contains("document.querySelectorAll(\"form\")"));
        assert!(script.contains("visibleText.slice(0, limit)"));
        assert_eq!(snapshot.url.as_ref(), "https://example.test/docs");
        assert_eq!(snapshot.title.as_ref(), "Docs");
        assert_eq!(snapshot.ready_state.as_ref(), "complete");
        assert_eq!(snapshot.language.as_ref().unwrap().as_ref(), "en");
        assert_eq!(snapshot.direction.as_ref().unwrap().as_ref(), "ltr");
        assert_eq!(snapshot.visible_text.as_ref(), "Docs Welcome Open settings");
        assert_eq!(snapshot.text_length, 26);
        assert_eq!(snapshot.headings[0].level, 1);
        assert_eq!(snapshot.headings[0].text.as_ref(), "Docs");
        assert_eq!(snapshot.headings[0].id.as_ref().unwrap().as_ref(), "top");
        assert_eq!(
            snapshot.links[0].target.as_ref().unwrap().as_ref(),
            "_blank"
        );
        assert_eq!(snapshot.images[0].alt.as_ref(), "Logo");
        assert_eq!(snapshot.forms[0].control_count, 2);
        assert!(parse_webview_document_snapshot_result("\"not snapshot\"").is_err());
    }

    #[test]
    fn webview_element_snapshot_script_serializes_selected_element_state() {
        let script = webview_element_snapshot_script("button[data-id=\"42\"]");
        let snapshot = parse_webview_element_snapshot_result(
            r##"{"url":"https://example.test/app","selector":"button.primary","tagName":"button","id":"save","classes":["primary","busy"],"text":"Save now","value":"save","checked":null,"disabled":false,"hidden":false,"editable":false,"href":null,"src":null,"rect":{"x":10.5,"y":20.0,"width":120.0,"height":32.0},"attributes":[{"name":"data-id","value":"42"},{"name":"aria-busy","value":"true"}],"display":"inline-flex","visibility":"visible","pointerEvents":"auto"}"##,
        )
        .unwrap()
        .expect("element snapshot");
        let missing = parse_webview_element_snapshot_result("null").unwrap();

        assert!(script.contains("document.querySelector(selector)"));
        assert!(script.contains("button[data-id=\\\"42\\\"]"));
        assert!(script.contains("element.getBoundingClientRect"));
        assert!(script.contains("Array.from(element.attributes || [])"));
        assert!(script.contains("window.getComputedStyle"));
        assert!(script.contains("pointerEvents"));
        assert_eq!(snapshot.url.as_ref(), "https://example.test/app");
        assert_eq!(snapshot.tag_name.as_ref(), "button");
        assert_eq!(snapshot.id.as_ref().unwrap().as_ref(), "save");
        assert_eq!(snapshot.classes.len(), 2);
        assert_eq!(snapshot.text.as_ref(), "Save now");
        assert_eq!(snapshot.value.as_ref().unwrap().as_ref(), "save");
        assert_eq!(snapshot.rect.width, 120.0);
        assert_eq!(snapshot.attributes[0].name.as_ref(), "data-id");
        assert_eq!(snapshot.pointer_events.as_ref(), "auto");
        assert!(missing.is_none());
        assert!(parse_webview_element_snapshot_result("\"not snapshot\"").is_err());
    }

    #[test]
    fn webview_capture_dom_image_script_serializes_same_document_element() {
        let script = webview_capture_dom_image_script(
            "main.receipt",
            &WebViewDomImageCaptureOptions::default()
                .size(640, 480)
                .background("#fff")
                .max_pixels(100_000),
        );

        assert!(script.contains("document.querySelector(selector)"));
        assert!(script.contains("main.receipt"));
        assert!(script.contains("\"width\":640"));
        assert!(script.contains("\"height\":480"));
        assert!(script.contains("\"background\":\"#fff\""));
        assert!(script.contains("\"maxPixels\":100000"));
        assert!(script.contains("target.cloneNode(true)"));
        assert!(script.contains("window.getComputedStyle(source)"));
        assert!(script.contains("copy.setAttribute(\"style\", css)"));
        assert!(script.contains("source instanceof HTMLInputElement"));
        assert!(script.contains("source instanceof HTMLTextAreaElement"));
        assert!(script.contains("source instanceof HTMLSelectElement"));
        assert!(script.contains("Math.sqrt(maxPixels / pixels)"));
        assert!(script.contains("new XMLSerializer().serializeToString(clone)"));
        assert!(script.contains("<foreignObject"));
        assert!(script.contains("data:image/svg+xml;charset=utf-8"));
        assert!(script.contains("return null"));
    }

    #[test]
    fn webview_download_trigger_script_clicks_browser_download_anchor() {
        let script = webview_trigger_download_script(
            "../media/demo video.mp4?download=1",
            Some(&SharedString::from("demo video.mp4")),
        );
        let result = parse_webview_download_trigger_result(
            r#"{"ok":true,"url":"https://example.test/media/demo%20video.mp4?download=1","filename":"demo video.mp4","error":null}"#,
        )
        .unwrap();
        let failed = parse_webview_download_trigger_result(
            r#"{"ok":false,"url":"://bad","filename":null,"error":"Invalid URL"}"#,
        )
        .unwrap();

        assert!(script.contains("new URL(String(requestedUrl || \"\"), document.baseURI"));
        assert!(script.contains("document.createElement(\"a\")"));
        assert!(script.contains("anchor.download = String(requestedFilename)"));
        assert!(script.contains("anchor.click()"));
        assert!(script.contains("anchor.remove()"));
        assert!(script.contains("../media/demo video.mp4?download=1"));
        assert!(script.contains("demo video.mp4"));
        assert!(result.ok);
        assert_eq!(
            result.url.as_ref(),
            "https://example.test/media/demo%20video.mp4?download=1"
        );
        assert_eq!(result.filename.unwrap().as_ref(), "demo video.mp4");
        assert!(!failed.ok);
        assert_eq!(failed.error.unwrap().as_ref(), "Invalid URL");
        assert!(parse_webview_download_trigger_result("\"not a result\"").is_err());
    }

    #[test]
    fn webview_favicons_script_reads_icon_links() {
        let script = webview_favicons_script();

        assert!(script.contains("link[rel~='icon'][href]"));
        assert!(script.contains("link[rel='shortcut icon'][href]"));
        assert!(script.contains("link[rel~='apple-touch-icon'][href]"));
        assert!(script.contains("link[rel~='mask-icon'][href]"));
        assert!(script.contains("document.querySelectorAll"));
        assert!(script.contains("seen"));
        assert_eq!(
            parse_webview_string_array_result(
                r#"["https://example.test/favicon.ico","https://example.test/icon.svg"]"#
            )
            .unwrap(),
            vec![
                SharedString::from("https://example.test/favicon.ico"),
                SharedString::from("https://example.test/icon.svg")
            ]
        );
        assert!(parse_webview_string_array_result("\"not an array\"").is_err());
    }

    #[test]
    fn webview_favicon_bridge_script_forwards_icon_changes() {
        let script = webview_favicon_bridge_script("favicon:changed \"quoted\"");

        assert!(script.contains("\"favicon:changed \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("readFavicons"));
        assert!(script.contains("post({ urls })"));
        assert!(script.contains("MutationObserver"));
        assert!(script.contains("attributeFilter: [\"href\", \"rel\"]"));
        assert!(script.contains("pageshow"));
        assert!(script.contains("__kaelFaviconBridge"));
    }

    #[test]
    fn webview_console_bridge_script_forwards_console_and_errors() {
        let script = webview_console_bridge_script("console:message \"quoted\"");

        assert!(script.contains("\"console:message \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(
            script.contains("const methods = [\"debug\", \"log\", \"info\", \"warn\", \"error\"]")
        );
        assert!(script.contains("console[level] = (...args)"));
        assert!(script.contains("return original(...args)"));
        assert!(script.contains("window.addEventListener(\"error\""));
        assert!(script.contains("window.addEventListener(\"unhandledrejection\""));
        assert!(script.contains("message: values.map(textValue).join(\" \")"));
        assert!(script.contains("args: values.map(safeValue)"));
        assert!(script.contains("__kaelConsoleBridge"));
    }

    #[test]
    fn webview_keyboard_bridge_script_forwards_key_and_beforeinput_events() {
        let script = webview_keyboard_bridge_script("keyboard:event \"quoted\"");

        assert!(script.contains("\"keyboard:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("document.addEventListener(\"keydown\""));
        assert!(script.contains("document.addEventListener(\"keyup\""));
        assert!(script.contains("document.addEventListener(\"beforeinput\""));
        assert!(script.contains("key: event.key || null"));
        assert!(script.contains("code: event.code || null"));
        assert!(script.contains("targetEditable: editable(event.target)"));
        assert!(script.contains("inputType: event.inputType || null"));
        assert!(script.contains("defaultPrevented: !!event.defaultPrevented"));
        assert!(script.contains("__kaelKeyboardBridge"));
    }

    #[test]
    fn webview_location_bridge_script_forwards_spa_route_changes() {
        let script = webview_location_bridge_script("location:changed \"quoted\"");

        assert!(script.contains("\"location:changed \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("wrapHistory(\"pushState\")"));
        assert!(script.contains("wrapHistory(\"replaceState\")"));
        assert!(script.contains("window.addEventListener(\"popstate\""));
        assert!(script.contains("window.addEventListener(\"hashchange\""));
        assert!(script.contains("window.addEventListener(\"pageshow\""));
        assert!(script.contains("document.addEventListener(\"DOMContentLoaded\""));
        assert!(script.contains("url: location.href || \"\""));
        assert!(script.contains("title: document.title || \"\""));
        assert!(script.contains("readyState: document.readyState || \"\""));
        assert!(script.contains("canGoForward: !!(state && state.canGoForward)"));
        assert!(script.contains("__kaelLocationBridge"));
    }

    #[test]
    fn webview_lifecycle_bridge_script_forwards_focus_visibility_and_fullscreen() {
        let script = webview_lifecycle_bridge_script("lifecycle:event \"quoted\"");

        assert!(script.contains("\"lifecycle:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("window.addEventListener(\"focus\""));
        assert!(script.contains("window.addEventListener(\"blur\""));
        assert!(script.contains("document.addEventListener(\"visibilitychange\""));
        assert!(script.contains("window.addEventListener(\"pageshow\""));
        assert!(script.contains("window.addEventListener(\"pagehide\""));
        assert!(script.contains("document.addEventListener(\"fullscreenchange\""));
        assert!(script.contains("document.addEventListener(\"webkitfullscreenchange\""));
        assert!(script.contains("document.visibilityState || \"\""));
        assert!(script.contains("hidden: !!document.hidden"));
        assert!(script.contains("document.hasFocus()"));
        assert!(script.contains("fullscreen: isFullscreen()"));
        assert!(script.contains("emit(\"initial\")"));
        assert!(script.contains("__kaelLifecycleBridge"));
    }

    #[test]
    fn webview_scroll_bridge_script_forwards_viewport_scroll_snapshots() {
        let script = webview_scroll_bridge_script("scroll:event \"quoted\"");

        assert!(script.contains("\"scroll:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("document.scrollingElement"));
        assert!(script.contains("window.innerWidth"));
        assert!(script.contains("window.innerHeight"));
        assert!(script.contains("window.scrollX"));
        assert!(script.contains("window.scrollY"));
        assert!(script.contains("progressX: maxX > 0 ? x / maxX : 0"));
        assert!(script.contains("progressY: maxY > 0 ? y / maxY : 0"));
        assert!(script.contains("requestAnimationFrame(flush)"));
        assert!(script.contains("window.addEventListener(\"scroll\""));
        assert!(script.contains("window.addEventListener(\"resize\""));
        assert!(script.contains("schedule(\"initial\")"));
        assert!(script.contains("__kaelScrollBridge"));
    }

    #[test]
    fn webview_viewport_snapshot_and_scroll_scripts_return_scroll_snapshots() {
        let snapshot = webview_viewport_snapshot_script("snapshot");
        let scroll_to = webview_scroll_to_script(12.5, f64::NAN);
        let scroll_by = webview_scroll_by_script(-20.0, 64.0);
        let selector = webview_scroll_selector_into_view_script("main .target");
        let event = parse_webview_scroll_event_result(
            r#"{"event":"scroll","x":10,"y":20,"maxX":100,"maxY":200,"viewportWidth":800,"viewportHeight":600,"scrollWidth":900,"scrollHeight":800,"progressX":0.1,"progressY":0.1}"#,
        )
        .unwrap();
        let optional = parse_webview_optional_scroll_event_result(
            r#"{"event":"scrollIntoView","x":0,"y":50,"maxX":0,"maxY":100,"viewportWidth":320,"viewportHeight":480,"scrollWidth":320,"scrollHeight":580,"progressX":0,"progressY":0.5}"#,
        )
        .unwrap();

        assert!(snapshot.contains("return snapshot(\"snapshot\")"));
        assert!(snapshot.contains("document.scrollingElement"));
        assert!(snapshot.contains("progressY: maxY > 0 ? y / maxY : 0"));
        assert!(scroll_to.contains("window.scrollTo({ left: targetX, top: targetY"));
        assert!(scroll_to.contains("const targetX = 12.5"));
        assert!(scroll_to.contains("const targetY = 0"));
        assert!(scroll_to.contains("return snapshot(\"scroll\")"));
        assert!(scroll_by.contains("window.scrollBy({ left: deltaX, top: deltaY"));
        assert!(scroll_by.contains("const deltaX = -20"));
        assert!(scroll_by.contains("const deltaY = 64"));
        assert!(selector.contains("document.querySelector(selector)"));
        assert!(selector.contains("main .target"));
        assert!(selector.contains("target.scrollIntoView"));
        assert!(selector.contains("return null"));
        assert!(selector.contains("return snapshot(\"scrollIntoView\")"));
        assert_eq!(event.event.as_ref(), "scroll");
        assert_eq!(event.y, 20.0);
        assert_eq!(event.max_y, 200.0);
        assert_eq!(event.viewport_height, 600.0);
        assert_eq!(event.progress_y, 0.1);
        assert_eq!(
            optional.expect("scroll snapshot").event.as_ref(),
            "scrollIntoView"
        );
        assert_eq!(
            parse_webview_optional_scroll_event_result("null").unwrap(),
            None
        );
        assert!(parse_webview_scroll_event_result("\"not scroll\"").is_err());
        assert!(parse_webview_optional_scroll_event_result("42").is_err());
    }

    #[test]
    fn webview_selection_bridge_script_forwards_selection_snapshots() {
        let script = webview_selection_bridge_script("selection:event \"quoted\"");

        assert!(script.contains("\"selection:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("document.activeElement"));
        assert!(script.contains("selectionStart"));
        assert!(script.contains("selectionEnd"));
        assert!(script.contains("selectedText: text"));
        assert!(script.contains("selectedHtml: container.innerHTML"));
        assert!(script.contains("collapsed: start === end"));
        assert!(script.contains("selection.rangeCount"));
        assert!(script.contains("document.addEventListener(\"selectionchange\""));
        assert!(script.contains("document.addEventListener(\"select\""));
        assert!(script.contains("requestAnimationFrame(flush)"));
        assert!(script.contains("schedule(\"initial\")"));
        assert!(script.contains("__kaelSelectionBridge"));
    }

    #[test]
    fn webview_runtime_css_scripts_upsert_and_remove_named_styles() {
        let insert = webview_insert_css_script("theme \"dark\"", "body { color: \"red\"; }");
        let remove = webview_remove_inserted_css_script("theme \"dark\"");

        assert!(insert.contains("data-kael-style-key"));
        assert!(insert.contains("document.createElement('style')"));
        assert!(insert.contains("style.textContent = css"));
        assert!(insert.contains("\"theme \\\"dark\\\"\""));
        assert!(insert.contains("body { color: \\\"red\\\"; }"));
        assert!(remove.contains("removeChild(style)"));
        assert!(remove.contains("\"theme \\\"dark\\\"\""));
    }

    #[test]
    fn webview_media_scripts_control_audio_and_video_elements() {
        let play = webview_play_media_script();
        let pause = webview_pause_media_script();
        let mute = webview_set_media_muted_script(true);
        let unmute = webview_set_media_muted_script(false);
        let quiet = webview_set_media_volume_script(-3.0);
        let loud = webview_set_media_volume_script(f32::INFINITY);
        let rate = webview_set_media_playback_rate_script(1.25);
        let fallback_rate = webview_set_media_playback_rate_script(f32::NAN);
        let seek = webview_seek_media_secs_script(42.5);
        let negative_seek = webview_seek_media_secs_script(-10.0);
        let scoped_play =
            webview_media_command_script("video#preview .surface", WebViewMediaCommand::Play);
        let scoped_pause =
            webview_media_command_script("video#preview", WebViewMediaCommand::Pause);
        let scoped_toggle =
            webview_media_command_script("video#preview", WebViewMediaCommand::TogglePlay);
        let scoped_stop = webview_media_command_script("video#preview", WebViewMediaCommand::Stop);
        let scoped_mute =
            webview_media_command_script("video#preview", WebViewMediaCommand::SetMuted(true));
        let scoped_volume =
            webview_media_command_script("video#preview", WebViewMediaCommand::SetVolume(2.0));
        let scoped_rate = webview_media_command_script(
            "video#preview",
            WebViewMediaCommand::SetPlaybackRate(f32::NAN),
        );
        let scoped_seek =
            webview_media_command_script("video#preview", WebViewMediaCommand::SeekSecs(-5.0));
        let source = webview_set_media_source_script(
            "video#preview > source",
            "https://example.test/movie.mp4?name=\"clip\"",
        );
        let options = webview_set_media_options_script(
            "video#preview .surface",
            &WebViewMediaElementOptions::default()
                .controls(true)
                .loop_enabled(true)
                .autoplay(false)
                .muted(true)
                .plays_inline(true)
                .poster("https://example.test/poster.jpg")
                .preload("metadata")
                .controls_list(["nodownload", "nofullscreen"])
                .disable_picture_in_picture(true),
        );
        let frame = webview_capture_media_frame_script(
            "video#preview .surface",
            &WebViewMediaFrameCaptureOptions::default()
                .width(320)
                .mime_type("image/jpeg")
                .quality(0.82),
        );
        let track = webview_add_media_text_track_script(
            "video#preview .surface",
            &WebViewMediaTextTrackOptions::webvtt("https://example.test/captions.vtt")
                .id("en")
                .label("English")
                .language("en-US")
                .default_track(true)
                .mode("showing"),
        );
        let remove_track = webview_remove_media_text_track_script("video#preview", "en");
        let select_track = webview_select_media_text_track_script("en \"quoted\"");
        let disable_tracks = webview_disable_media_text_tracks_script();
        let request_fullscreen = webview_request_media_fullscreen_script();
        let exit_fullscreen = webview_exit_media_fullscreen_script();
        let request_pip = webview_request_media_picture_in_picture_script();
        let exit_pip = webview_exit_media_picture_in_picture_script();

        assert!(play.contains("querySelectorAll('audio,video')"));
        assert!(play.contains("element.play()"));
        assert!(play.contains("result.catch(() => {})"));
        assert!(pause.contains("querySelectorAll('audio,video')"));
        assert!(pause.contains("element.pause()"));
        assert!(mute.contains("element.muted = true"));
        assert!(unmute.contains("element.muted = false"));
        assert!(quiet.contains("const value = 0"));
        assert!(loud.contains("const value = 1"));
        assert!(rate.contains("element.playbackRate = value"));
        assert!(rate.contains("const value = 1.25"));
        assert!(fallback_rate.contains("const value = 1"));
        assert!(seek.contains("element.currentTime = value"));
        assert!(seek.contains("const value = 42.5"));
        assert!(negative_seek.contains("const value = 0"));
        assert!(scoped_play.contains("document.querySelector(selector)"));
        assert!(scoped_play.contains("video#preview .surface"));
        assert!(scoped_play.contains("\"action\":\"play\""));
        assert!(scoped_play.contains("target.closest(\"audio,video\")"));
        assert!(scoped_play.contains("safePlay()"));
        assert!(scoped_pause.contains("\"action\":\"pause\""));
        assert!(scoped_toggle.contains("\"action\":\"togglePlay\""));
        assert!(scoped_toggle.contains("if (media.paused) safePlay()"));
        assert!(scoped_stop.contains("\"action\":\"stop\""));
        assert!(scoped_stop.contains("media.currentTime = 0"));
        assert!(scoped_mute.contains("\"action\":\"setMuted\""));
        assert!(scoped_mute.contains("\"value\":true"));
        assert!(scoped_mute.contains("media.muted = !!command.value"));
        assert!(scoped_volume.contains("\"action\":\"setVolume\""));
        assert!(scoped_volume.contains("\"value\":1"));
        assert!(scoped_volume.contains("media.volume = Math.min(1"));
        assert!(scoped_rate.contains("\"action\":\"setPlaybackRate\""));
        assert!(scoped_rate.contains("\"value\":1"));
        assert!(scoped_rate.contains("media.playbackRate = Math.max(0"));
        assert!(scoped_seek.contains("\"action\":\"seek\""));
        assert!(scoped_seek.contains("\"value\":0"));
        assert!(scoped_seek.contains("media.currentTime = Math.max(0"));
        assert!(source.contains("document.querySelector(selector)"));
        assert!(source.contains("video#preview > source"));
        assert!(source.contains("https://example.test/movie.mp4?name=\\\"clip\\\""));
        assert!(source.contains("target.closest(\"audio,video\")"));
        assert!(source.contains("media.src = source"));
        assert!(source.contains("media.load()"));
        assert!(options.contains("document.querySelector(selector)"));
        assert!(options.contains("video#preview .surface"));
        assert!(options.contains("\"controls\":true"));
        assert!(options.contains("\"loop\":true"));
        assert!(options.contains("\"playsInline\":true"));
        assert!(options.contains("\"controlsList\":[\"nodownload\",\"nofullscreen\"]"));
        assert!(options.contains("target.closest(\"audio,video\")"));
        assert!(options.contains("media.controlsList.value = tokens.join(\" \")"));
        assert!(options.contains("media.poster = options.poster"));
        assert!(options.contains("media.preload = options.preload"));
        assert!(options.contains("boolProp(\"disablePictureInPicture\""));
        assert!(frame.contains("document.querySelector(selector)"));
        assert!(frame.contains("video#preview .surface"));
        assert!(frame.contains("\"mimeType\":\"image/jpeg\""));
        assert!(frame.contains("\"quality\":0.82"));
        assert!(frame.contains("target.closest(\"video\")"));
        assert!(frame.contains("!video.videoWidth || !video.videoHeight"));
        assert!(frame.contains("document.createElement(\"canvas\")"));
        assert!(frame.contains("context.drawImage(video"));
        assert!(frame.contains("canvas.toDataURL(mimeType, quality)"));
        assert!(frame.contains("return null"));
        assert!(track.contains("document.querySelector(selector)"));
        assert!(track.contains("video#preview .surface"));
        assert!(track.contains("https://example.test/captions.vtt"));
        assert!(track.contains("document.createElement(\"track\")"));
        assert!(track.contains("track.kind"));
        assert!(track.contains("track.srclang = trackOptions.language"));
        assert!(track.contains("media.appendChild(track)"));
        assert!(track.contains("track.track.mode = trackOptions.mode"));
        assert!(track.contains("track.addEventListener(\"load\", applyMode"));
        assert!(remove_track.contains("document.querySelector(selector)"));
        assert!(remove_track.contains("video#preview"));
        assert!(remove_track.contains("const trackSelector = \"en\""));
        assert!(remove_track.contains("media.querySelectorAll(\"track\")"));
        assert!(remove_track.contains("String(index) === trackSelector"));
        assert!(remove_track.contains("element.srclang === trackSelector"));
        assert!(remove_track.contains("element.src === trackSelector"));
        assert!(remove_track.contains("browserTrack.language === trackSelector"));
        assert!(remove_track.contains("browserTrack.mode = \"disabled\""));
        assert!(remove_track.contains("element.parentNode.removeChild(element)"));
        assert!(select_track.contains("const selector = \"en \\\"quoted\\\"\""));
        assert!(select_track.contains("element.textTracks"));
        assert!(select_track.contains("track.id === selector"));
        assert!(select_track.contains("track.label === selector"));
        assert!(select_track.contains("track.language === selector"));
        assert!(select_track.contains("String(index) === selector"));
        assert!(select_track.contains("track.mode = matches ? 'showing' : 'disabled'"));
        assert!(disable_tracks.contains("element.textTracks"));
        assert!(disable_tracks.contains("track.mode = 'disabled'"));
        assert!(request_fullscreen.contains("document.querySelector('video')"));
        assert!(request_fullscreen.contains("requestFullscreen"));
        assert!(request_fullscreen.contains("webkitRequestFullscreen"));
        assert!(request_fullscreen.contains("result.catch(() => {})"));
        assert!(exit_fullscreen.contains("document.exitFullscreen"));
        assert!(exit_fullscreen.contains("webkitExitFullscreen"));
        assert!(request_pip.contains("requestPictureInPicture"));
        assert!(request_pip.contains("result.catch(() => {})"));
        assert!(exit_pip.contains("document.pictureInPictureElement"));
        assert!(exit_pip.contains("document.exitPictureInPicture"));
    }

    #[test]
    fn webview_media_state_script_snapshots_audio_and_video_elements() {
        let script = webview_media_state_script();
        let states = parse_webview_media_state_result(
            r#"[{"index":0,"tagName":"video","id":"preview","src":"https://example.test/movie.mp4","paused":false,"ended":false,"muted":true,"volume":0.5,"playbackRate":1.25,"currentTime":42.5,"duration":120.0,"readyState":4,"networkState":1,"seeking":false,"fullscreen":true,"pictureInPicture":true,"buffered":[{"start":0.0,"end":60.0}],"textTracks":[{"index":0,"id":"en","kind":"subtitles","label":"English","language":"en","mode":"showing","activeCues":[{"id":"cue-1","startTime":40.0,"endTime":44.0,"text":"Hello there"}]}]},{"index":1,"tagName":"audio","id":null,"src":null,"paused":true,"ended":true,"muted":false,"volume":1.0,"playbackRate":1.0,"currentTime":0.0,"duration":null,"readyState":0,"networkState":0,"seeking":false,"fullscreen":false,"pictureInPicture":false,"buffered":[],"textTracks":[]}]"#,
        )
        .unwrap();

        assert!(script.contains("querySelectorAll('audio,video')"));
        assert!(script.contains("currentSrc"));
        assert!(script.contains("playbackRate"));
        assert!(script.contains("document.fullscreenElement === element"));
        assert!(script.contains("document.pictureInPictureElement === element"));
        assert!(script.contains("Array.from(element.textTracks || [])"));
        assert!(script.contains("activeCues"));
        assert!(script.contains("buffered.push"));
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].tag_name.as_ref(), "video");
        assert_eq!(states[0].id.as_ref().unwrap().as_ref(), "preview");
        assert_eq!(
            states[0].src.as_ref().unwrap().as_ref(),
            "https://example.test/movie.mp4"
        );
        assert!(!states[0].paused);
        assert!(states[0].muted);
        assert_eq!(states[0].volume, 0.5);
        assert_eq!(states[0].playback_rate, 1.25);
        assert_eq!(states[0].current_time, 42.5);
        assert_eq!(states[0].duration, Some(120.0));
        assert_eq!(states[0].ready_state, 4);
        assert!(states[0].fullscreen);
        assert!(states[0].picture_in_picture);
        assert_eq!(states[0].buffered[0].end, 60.0);
        assert_eq!(states[0].text_tracks[0].id.as_ref().unwrap().as_ref(), "en");
        assert_eq!(states[0].text_tracks[0].kind.as_ref(), "subtitles");
        assert_eq!(states[0].text_tracks[0].mode.as_ref(), "showing");
        assert_eq!(
            states[0].text_tracks[0].active_cues[0].text.as_ref(),
            "Hello there"
        );
        assert_eq!(states[1].tag_name.as_ref(), "audio");
        assert_eq!(states[1].duration, None);
        assert!(!states[1].fullscreen);
        assert!(!states[1].picture_in_picture);
        assert!(states[1].text_tracks.is_empty());
        assert!(parse_webview_media_state_result("\"not media\"").is_err());
    }

    #[test]
    fn webview_media_event_bridge_script_forwards_media_events() {
        let script = webview_media_event_bridge_script("media:event \"quoted\"");

        assert!(script.contains("\"media:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("MutationObserver"));
        assert!(script.contains("element.addEventListener(event"));
        assert!(script.contains("\"timeupdate\""));
        assert!(script.contains("\"volumechange\""));
        assert!(script.contains("\"ratechange\""));
        assert!(script.contains("\"loadedmetadata\""));
        assert!(script.contains("state: stateFor(element)"));
        assert!(script.contains("document.fullscreenElement === element"));
        assert!(script.contains("document.pictureInPictureElement === element"));
        assert!(script.contains("Array.from(element.textTracks || [])"));
        assert!(script.contains("activeCues"));
        assert!(script.contains("__kaelMediaEventBridge"));
    }

    #[test]
    fn webview_context_menu_bridge_script_forwards_page_context() {
        let script = webview_context_menu_bridge_script("context:menu \"quoted\"");

        assert!(script.contains("\"context:menu \\\"quoted\\\"\""));
        assert!(script.contains("document.addEventListener(\"contextmenu\""));
        assert!(script.contains("event.preventDefault()"));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("selectedText: selectedText()"));
        assert!(script.contains("linkHref: link ? link.href : null"));
        assert!(script.contains("imageSrc: image ?"));
        assert!(script.contains("mediaSrc: media ?"));
        assert!(script.contains("editable: !!"));
        assert!(script.contains("inputKind: input ?"));
        assert!(script.contains("__kaelContextMenuBridge"));
    }

    #[test]
    fn webview_pointer_bridge_script_forwards_hover_and_click_context() {
        let script = webview_pointer_bridge_script("pointer:event \"quoted\"");

        assert!(script.contains("\"pointer:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("document.addEventListener(\"pointermove\""));
        assert!(script.contains("document.addEventListener(\"pointerdown\""));
        assert!(script.contains("document.addEventListener(\"pointerup\""));
        assert!(script.contains("document.addEventListener(\"click\""));
        assert!(script.contains("document.addEventListener(\"dblclick\""));
        assert!(script.contains("requestAnimationFrame(flushMove)"));
        assert!(script.contains("linkHref: link ? link.href : null"));
        assert!(script.contains("imageSrc: image ?"));
        assert!(script.contains("mediaSrc: media ?"));
        assert!(script.contains("targetTag: element ?"));
        assert!(script.contains("pointerType: event.pointerType"));
        assert!(script.contains("__kaelPointerBridge"));
    }

    #[test]
    fn webview_form_bridge_script_forwards_form_activity() {
        let script = webview_form_bridge_script("form:event \"quoted\"");

        assert!(script.contains("\"form:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("document.addEventListener(\"submit\""));
        assert!(script.contains("document.addEventListener(\"reset\""));
        assert!(script.contains("document.addEventListener(\"change\""));
        assert!(script.contains("document.addEventListener(\"input\""));
        assert!(script.contains("event.submitter"));
        assert!(script.contains("type === \"password\" || type === \"file\""));
        assert!(script.contains("fields: includeFields ? controls(form) : []"));
        assert!(script.contains("defaultPrevented: !!event.defaultPrevented"));
        assert!(script.contains("__kaelFormBridge"));
    }

    #[test]
    fn webview_file_input_bridge_script_forwards_file_selection_metadata() {
        let script = webview_file_input_bridge_script("file:event \"quoted\"");

        assert!(script.contains("\"file:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("String(element.type || \"\").toLowerCase() === \"file\""));
        assert!(script.contains("document.addEventListener(\"change\""));
        assert!(script.contains("document.addEventListener(\"input\""));
        assert!(script.contains("name: String(file.name || \"\")"));
        assert!(script.contains("size: Number.isFinite(file.size) ? file.size : 0"));
        assert!(script.contains("mimeType: nullable(file.type)"));
        assert!(script.contains("lastModified: Number.isFinite(file.lastModified)"));
        assert!(script.contains("files: Array.from(input.files || []).map(fileState)"));
        assert!(script.contains("__kaelFileInputBridge"));
    }

    #[test]
    fn webview_resource_bridge_script_forwards_timing_and_element_events() {
        let script = webview_resource_bridge_script("resource:event \"quoted\"");

        assert!(script.contains("\"resource:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("performance.getEntriesByType(\"resource\")"));
        assert!(script.contains("new PerformanceObserver"));
        assert!(script.contains("observer.observe({ type: \"resource\", buffered: true })"));
        assert!(script.contains("document.addEventListener(\"load\""));
        assert!(script.contains("document.addEventListener(\"error\""));
        assert!(script.contains("initiatorType: String(entry.initiatorType || \"\")"));
        assert!(script.contains("transferSize: finite(entry.transferSize)"));
        assert!(script.contains("renderBlockingStatus: nullable(entry.renderBlockingStatus)"));
        assert!(script.contains("success: event.type === \"load\" ? true"));
        assert!(script.contains("__kaelResourceBridge"));
    }

    #[test]
    fn webview_network_bridge_script_forwards_fetch_and_xhr_outcomes() {
        let script = webview_network_bridge_script("network:event \"quoted\"");

        assert!(script.contains("\"network:event \\\"quoted\\\"\""));
        assert!(script.contains("__kaelNetworkBridge"));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("const originalFetch = window.fetch.bind(window)"));
        assert!(script.contains("window.fetch = (input, init)"));
        assert!(script.contains("fetch-error"));
        assert!(script.contains("const OriginalXHR = window.XMLHttpRequest"));
        assert!(script.contains("window.XMLHttpRequest = function KaelXMLHttpRequest()"));
        assert!(script.contains("xhr.addEventListener(\"load\""));
        assert!(script.contains("xhr.addEventListener(\"error\""));
        assert!(script.contains("xhr.addEventListener(\"abort\""));
        assert!(script.contains("xhr.addEventListener(\"timeout\""));
        assert!(script.contains("durationMs"));
        assert!(script.contains("documentUrl"));
    }

    #[test]
    fn webview_dialog_bridge_script_forwards_browser_dialogs() {
        let script = webview_dialog_bridge_script("dialog:event \"quoted\"");

        assert!(script.contains("\"dialog:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("const originalAlert = window.alert"));
        assert!(script.contains("window.alert = (message)"));
        assert!(script.contains("window.confirm = (message)"));
        assert!(script.contains("window.prompt = (message, defaultValue = \"\")"));
        assert!(script.contains("post(payload(\"confirm\", message, null, !!result))"));
        assert!(script.contains("post(payload(\"prompt\", message, defaultValue"));
        assert!(script.contains("window.addEventListener(\"beforeunload\""));
        assert!(script.contains("__kaelDialogBridge"));
    }

    #[test]
    fn webview_clipboard_event_bridge_script_forwards_copy_cut_paste() {
        let script = webview_clipboard_event_bridge_script("clipboard:event \"quoted\"");

        assert!(script.contains("\"clipboard:event \\\"quoted\\\"\""));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("event.clipboardData || window.clipboardData"));
        assert!(script.contains("data.getData(type)"));
        assert!(script.contains("text: readData(data, \"text/plain\")"));
        assert!(script.contains("html: readData(data, \"text/html\")"));
        assert!(script.contains("document.addEventListener(\"copy\""));
        assert!(script.contains("document.addEventListener(\"cut\""));
        assert!(script.contains("document.addEventListener(\"paste\""));
        assert!(script.contains("defaultPrevented: !!event.defaultPrevented"));
        assert!(script.contains("__kaelClipboardEventBridge"));
    }

    #[test]
    fn webview_permission_bridge_script_preflights_browser_permission_apis() {
        let script = webview_permission_bridge_script("permission:request \"quoted\"");

        assert!(script.contains("\"permission:request \\\"quoted\\\"\""));
        assert!(script.contains("__kaelPermissionBridge"));
        assert!(script.contains("window.kael.invoke(kind"));
        assert!(script.contains("navigator.mediaDevices.getUserMedia"));
        assert!(script.contains("permissions.push(\"camera\")"));
        assert!(script.contains("permissions.push(\"microphone\")"));
        assert!(script.contains("navigator.mediaDevices.getDisplayMedia"));
        assert!(script.contains("permissions.push(\"system-audio\")"));
        assert!(script.contains("mediaDevices.getDisplayMedia"));
        assert!(script.contains("display-capture"));
        assert!(script.contains("navigator.geolocation.getCurrentPosition"));
        assert!(script.contains("navigator.geolocation.watchPosition"));
        assert!(script.contains("Notification.requestPermission"));
        assert!(script.contains("decision === \"deny\""));
        assert!(script.contains("NotAllowedError"));
        assert!(script.contains("userGesture"));
    }

    #[test]
    fn webview_storage_bridge_script_forwards_storage_changes() {
        let script = webview_storage_bridge_script("storage:event \"quoted\"");

        assert!(script.contains("\"storage:event \\\"quoted\\\"\""));
        assert!(script.contains("__kaelStorageBridge"));
        assert!(script.contains("window.kael.post(kind, payload)"));
        assert!(script.contains("window.gpui.postMessage({ kind, payload })"));
        assert!(script.contains("window.external.invoke({ kind, payload })"));
        assert!(script.contains("wrapArea(\"localStorage\", window.localStorage)"));
        assert!(script.contains("wrapArea(\"sessionStorage\", window.sessionStorage)"));
        assert!(script.contains("storage.setItem = (key, value)"));
        assert!(script.contains("storage.removeItem = (key)"));
        assert!(script.contains("storage.clear = ()"));
        assert!(script.contains("window.addEventListener(\"storage\""));
        assert!(script.contains("oldValue"));
        assert!(script.contains("newValue"));
        assert!(script.contains("local: !!local"));
    }

    #[test]
    fn webview_storage_snapshot_script_reads_local_and_session_storage() {
        let script = webview_storage_snapshot_script();
        let snapshot = parse_webview_storage_snapshot_result(
            r#"{"url":"https://example.test/settings","origin":"https://example.test","localStorage":{"area":"localStorage","available":true,"length":2,"entries":[{"key":"theme","value":"dark"},{"key":"draft","value":"yes"}],"error":null},"sessionStorage":{"area":"sessionStorage","available":false,"length":0,"entries":[],"error":"blocked"}}"#,
        )
        .unwrap();

        assert!(script.contains("readArea(\"localStorage\", window.localStorage)"));
        assert!(script.contains("readArea(\"sessionStorage\", window.sessionStorage)"));
        assert!(script.contains("storage.key(index)"));
        assert!(script.contains("storage.getItem(key)"));
        assert!(script.contains("available: false"));
        assert_eq!(snapshot.url.as_ref(), "https://example.test/settings");
        assert_eq!(snapshot.origin.as_ref(), "https://example.test");
        assert!(snapshot.local_storage.available);
        assert_eq!(snapshot.local_storage.length, 2);
        assert_eq!(snapshot.local_storage.entries[0].key.as_ref(), "theme");
        assert_eq!(snapshot.local_storage.entries[0].value.as_ref(), "dark");
        assert!(!snapshot.session_storage.available);
        assert_eq!(
            snapshot.session_storage.error.as_ref().unwrap().as_ref(),
            "blocked"
        );
        assert!(parse_webview_storage_snapshot_result("\"not storage\"").is_err());
    }

    #[test]
    fn webview_storage_mutation_scripts_report_structured_results() {
        let set = webview_set_storage_item_script(WebViewStorageArea::Local, "theme", "dark");
        let remove = webview_remove_storage_item_script(WebViewStorageArea::Session, "draft");
        let clear = webview_clear_storage_area_script(WebViewStorageArea::Local);
        let result = parse_webview_storage_mutation_result(
            r#"{"ok":true,"area":"localStorage","key":"theme","length":1,"error":null}"#,
        )
        .unwrap();
        let failure = parse_webview_storage_mutation_result(
            r#"{"ok":false,"area":"sessionStorage","key":"draft","length":0,"error":"blocked"}"#,
        )
        .unwrap();

        assert!(set.contains("const action = \"setItem\""));
        assert!(set.contains("const area = \"localStorage\""));
        assert!(set.contains("const key = \"theme\""));
        assert!(set.contains("const value = \"dark\""));
        assert!(set.contains("storage.setItem(String(key), String(value))"));
        assert!(remove.contains("const action = \"removeItem\""));
        assert!(remove.contains("const area = \"sessionStorage\""));
        assert!(remove.contains("storage.removeItem(String(key))"));
        assert!(clear.contains("const action = \"clear\""));
        assert!(clear.contains("storage.clear()"));
        assert!(set.contains("ok: false"));
        assert!(set.contains("error && error.message"));
        assert!(result.ok);
        assert_eq!(result.area.as_ref(), "localStorage");
        assert_eq!(result.key.as_ref().unwrap().as_ref(), "theme");
        assert_eq!(result.length, 1);
        assert!(!failure.ok);
        assert_eq!(failure.error.as_ref().unwrap().as_ref(), "blocked");
        assert!(parse_webview_storage_mutation_result("\"not mutation\"").is_err());
    }

    #[test]
    fn webview_options_can_override_media_autoplay() {
        let options = WebViewOptions::embedded_widget().media_autoplay();
        assert_eq!(options.media_autoplay_requested(), Some(true));

        let element = webview_with_options(
            "media-docs",
            "https://example.com",
            WebViewOptions::embedded_widget().media_autoplay_enabled(false),
        );
        assert_eq!(element.options.media_autoplay_requested(), Some(false));

        let enabled = element.media_autoplay_enabled(true);
        assert_eq!(enabled.options.media_autoplay_requested(), Some(true));
    }

    #[test]
    fn webview_options_can_inject_media_event_bridge() {
        let options = WebViewOptions::embedded_widget().media_event_bridge("media:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"media:event\""))
        );

        let element = webview("media", "https://example.com").media_event_bridge("player:update");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"player:update\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_media_events() {
        let options =
            WebViewOptions::embedded_widget().on_media_event("media:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"media:event\""))
        );

        let element = webview("media", "https://example.com").on_media_event(
            "player:update",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"player:update\""))
        );
    }

    #[test]
    fn webview_options_can_inject_context_menu_bridge() {
        let options = WebViewOptions::embedded_widget().context_menu_bridge("context:menu");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"context:menu\""))
        );

        let element = webview("docs", "https://example.com").context_menu_bridge("menu:open");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"menu:open\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_context_menu_events() {
        let options =
            WebViewOptions::embedded_widget().on_context_menu("context:menu", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"context:menu\""))
        );

        let element =
            webview("docs", "https://example.com").on_context_menu("menu:open", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"menu:open\""))
        );
    }

    #[test]
    fn webview_options_can_inject_pointer_bridge() {
        let options = WebViewOptions::embedded_widget().pointer_bridge("pointer:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"pointer:event\""))
        );

        let element = webview("docs", "https://example.com").pointer_bridge("tab:pointer");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:pointer\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_pointer_events() {
        let options =
            WebViewOptions::embedded_widget().on_pointer_event("pointer:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"pointer:event\""))
        );

        let element = webview("docs", "https://example.com").on_pointer_event(
            "tab:pointer",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:pointer\""))
        );
    }

    #[test]
    fn webview_options_can_inject_form_bridge() {
        let options = WebViewOptions::embedded_widget().form_bridge("form:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"form:event\""))
        );

        let element = webview("checkout", "https://example.com").form_bridge("tab:form");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:form\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_form_events() {
        let options =
            WebViewOptions::embedded_widget().on_form_event("form:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"form:event\""))
        );

        let element =
            webview("checkout", "https://example.com").on_form_event("tab:form", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:form\""))
        );
    }

    #[test]
    fn webview_options_can_inject_file_input_bridge() {
        let options = WebViewOptions::embedded_widget().file_input_bridge("file:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"file:event\""))
        );

        let element = webview("upload", "https://example.com").file_input_bridge("tab:file");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:file\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_file_input_events() {
        let options =
            WebViewOptions::embedded_widget().on_file_input_event("file:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"file:event\""))
        );

        let element = webview("upload", "https://example.com").on_file_input_event(
            "tab:file",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:file\""))
        );
    }

    #[test]
    fn webview_options_can_inject_resource_bridge() {
        let options = WebViewOptions::embedded_widget().resource_bridge("resource:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"resource:event\""))
        );

        let element = webview("docs", "https://example.com").resource_bridge("tab:resource");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:resource\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_resource_events() {
        let options =
            WebViewOptions::embedded_widget().on_resource_event("resource:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"resource:event\""))
        );

        let element = webview("docs", "https://example.com").on_resource_event(
            "tab:resource",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:resource\""))
        );
    }

    #[test]
    fn webview_options_can_inject_network_bridge() {
        let options = WebViewOptions::embedded_widget().network_bridge("network:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"network:event\""))
        );

        let element = webview("api", "https://example.com").network_bridge("tab:network");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:network\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_network_events() {
        let options =
            WebViewOptions::embedded_widget().on_network_event("network:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"network:event\""))
        );

        let element =
            webview("api", "https://example.com").on_network_event("tab:network", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:network\""))
        );
    }

    #[test]
    fn webview_options_can_inject_dialog_bridge() {
        let options = WebViewOptions::embedded_widget().dialog_bridge("dialog:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"dialog:event\""))
        );

        let element = webview("docs", "https://example.com").dialog_bridge("tab:dialog");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:dialog\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_dialog_events() {
        let options =
            WebViewOptions::embedded_widget().on_dialog_event("dialog:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"dialog:event\""))
        );

        let element =
            webview("docs", "https://example.com").on_dialog_event("tab:dialog", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:dialog\""))
        );
    }

    #[test]
    fn webview_options_can_inject_clipboard_event_bridge() {
        let options = WebViewOptions::embedded_widget().clipboard_event_bridge("clipboard:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"clipboard:event\""))
        );

        let element =
            webview("editor", "https://example.com").clipboard_event_bridge("tab:clipboard");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:clipboard\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_clipboard_events() {
        let options = WebViewOptions::embedded_widget().on_clipboard_event(
            "clipboard:event",
            |event, _, _| {
                let _ = event;
            },
        );

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"clipboard:event\""))
        );

        let element = webview("editor", "https://example.com").on_clipboard_event(
            "tab:clipboard",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:clipboard\""))
        );
    }

    #[test]
    fn webview_options_can_inject_permission_bridge() {
        let options = WebViewOptions::embedded_widget().permission_bridge("permission:request");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"permission:request\""))
        );

        let element = webview("call", "https://example.com").permission_bridge("tab:permission");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:permission\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_permission_requests() {
        let options = WebViewOptions::embedded_widget().on_permission_request(
            "permission:request",
            |request, _, _| {
                let _ = request;
                WebViewPermissionDecision::Deny
            },
        );

        assert!(options.on_permission_request.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"permission:request\""))
        );

        let element = webview("call", "https://example.com").on_permission_request(
            "tab:permission",
            |request, _, _| {
                let _ = request;
                WebViewPermissionDecision::Allow
            },
        );
        assert!(element.options.on_permission_request.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:permission\""))
        );
    }

    #[test]
    fn webview_options_can_inject_storage_bridge() {
        let options = WebViewOptions::embedded_widget().storage_bridge("storage:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"storage:event\""))
        );

        let element = webview("settings", "https://example.com").storage_bridge("tab:storage");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:storage\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_storage_events() {
        let options =
            WebViewOptions::embedded_widget().on_storage_event("storage:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"storage:event\""))
        );

        let element = webview("settings", "https://example.com").on_storage_event(
            "tab:storage",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:storage\""))
        );
    }

    #[test]
    fn webview_options_can_inject_favicon_bridge() {
        let options = WebViewOptions::embedded_widget().favicon_bridge("favicon:changed");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"favicon:changed\""))
        );

        let element = webview("docs", "https://example.com").favicon_bridge("tab:favicon");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:favicon\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_favicon_events() {
        let options = WebViewOptions::embedded_widget().on_favicon_changed(
            "favicon:changed",
            |event, _, _| {
                let _ = event;
            },
        );

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"favicon:changed\""))
        );

        let element = webview("docs", "https://example.com").on_favicon_changed(
            "tab:favicon",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:favicon\""))
        );
    }

    #[test]
    fn webview_options_can_inject_console_bridge() {
        let options = WebViewOptions::embedded_widget().console_bridge("console:message");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"console:message\""))
        );

        let element = webview("docs", "https://example.com").console_bridge("tab:console");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:console\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_console_events() {
        let options = WebViewOptions::embedded_widget().on_console_message(
            "console:message",
            |event, _, _| {
                let _ = event;
            },
        );

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"console:message\""))
        );

        let element = webview("docs", "https://example.com").on_console_message(
            "tab:console",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:console\""))
        );
    }

    #[test]
    fn webview_options_can_inject_keyboard_event_bridge() {
        let options = WebViewOptions::embedded_widget().keyboard_event_bridge("keyboard:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"keyboard:event\""))
        );

        let element = webview("docs", "https://example.com").keyboard_event_bridge("tab:keys");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:keys\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_keyboard_events() {
        let options =
            WebViewOptions::embedded_widget().on_keyboard_event("keyboard:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"keyboard:event\""))
        );

        let element =
            webview("docs", "https://example.com").on_keyboard_event("tab:keys", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:keys\""))
        );
    }

    #[test]
    fn webview_options_can_inject_location_bridge() {
        let options = WebViewOptions::embedded_widget().location_bridge("location:changed");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"location:changed\""))
        );

        let element = webview("docs", "https://example.com").location_bridge("tab:location");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:location\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_location_events() {
        let options = WebViewOptions::embedded_widget().on_location_changed(
            "location:changed",
            |event, _, _| {
                let _ = event;
            },
        );

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"location:changed\""))
        );

        let element = webview("docs", "https://example.com").on_location_changed(
            "tab:location",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:location\""))
        );
    }

    #[test]
    fn webview_options_can_inject_lifecycle_bridge() {
        let options = WebViewOptions::embedded_widget().lifecycle_bridge("lifecycle:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"lifecycle:event\""))
        );

        let element = webview("docs", "https://example.com").lifecycle_bridge("tab:lifecycle");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:lifecycle\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_lifecycle_events() {
        let options = WebViewOptions::embedded_widget().on_lifecycle_event(
            "lifecycle:event",
            |event, _, _| {
                let _ = event;
            },
        );

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"lifecycle:event\""))
        );

        let element = webview("docs", "https://example.com").on_lifecycle_event(
            "tab:lifecycle",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:lifecycle\""))
        );
    }

    #[test]
    fn webview_options_can_inject_scroll_bridge() {
        let options = WebViewOptions::embedded_widget().scroll_bridge("scroll:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"scroll:event\""))
        );

        let element = webview("docs", "https://example.com").scroll_bridge("tab:scroll");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:scroll\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_scroll_events() {
        let options =
            WebViewOptions::embedded_widget().on_scroll_event("scroll:event", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"scroll:event\""))
        );

        let element =
            webview("docs", "https://example.com").on_scroll_event("tab:scroll", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:scroll\""))
        );
    }

    #[test]
    fn webview_options_can_inject_selection_bridge() {
        let options = WebViewOptions::embedded_widget().selection_bridge("selection:event");
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"selection:event\""))
        );

        let element = webview("docs", "https://example.com").selection_bridge("tab:selection");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:selection\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_selection_events() {
        let options = WebViewOptions::embedded_widget().on_selection_event(
            "selection:event",
            |event, _, _| {
                let _ = event;
            },
        );

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"selection:event\""))
        );

        let element = webview("docs", "https://example.com").on_selection_event(
            "tab:selection",
            |event, _, _| {
                let _ = event;
            },
        );
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:selection\""))
        );
    }

    #[test]
    fn webview_options_can_request_initial_focus() {
        let options = WebViewOptions::embedded_widget().focused();
        assert_eq!(options.focused_requested(), Some(true));

        let element = webview_with_options(
            "focused-auth",
            "https://example.com",
            WebViewOptions::embedded_widget().focused_enabled(false),
        );
        assert_eq!(element.options.focused_requested(), Some(false));

        let enabled = element.focused_enabled(true);
        assert_eq!(enabled.options.focused_requested(), Some(true));
    }

    #[test]
    fn webview_options_can_request_clipboard_access() {
        let options = WebViewOptions::embedded_widget().clipboard_access();
        assert!(options.clipboard_access_requested());

        let element = webview_with_options(
            "rich-editor",
            "https://example.com",
            WebViewOptions::embedded_widget().clipboard_access_enabled(false),
        );
        assert!(!element.options.clipboard_access_requested());

        let enabled = element.clipboard_access_enabled(true);
        assert!(enabled.options.clipboard_access_requested());
    }

    #[test]
    fn webview_options_can_request_initial_navigation_headers() {
        let mut headers = http_client::http::HeaderMap::new();
        headers.insert(
            http_client::http::header::ACCEPT_LANGUAGE,
            http_client::http::HeaderValue::from_static("en-US"),
        );
        let options = WebViewOptions::embedded_widget().request_headers(headers.clone());
        assert_eq!(options.request_headers_requested(), Some(&headers));

        let element = webview_with_options(
            "localized-docs",
            "https://example.com",
            WebViewOptions::embedded_widget().request_headers(headers.clone()),
        );
        assert_eq!(element.options.request_headers_requested(), Some(&headers));

        let cleared = element.clear_request_headers();
        assert!(cleared.options.request_headers_requested().is_none());
    }

    #[test]
    fn webview_options_can_request_initial_html() {
        let options = WebViewOptions::embedded_widget().html("<main>Preview</main>");
        assert_eq!(
            options.html_requested().map(|html| html.as_ref()),
            Some("<main>Preview</main>")
        );

        let element = webview("inline-html", "").html("<button>Save</button>");
        assert_eq!(
            element.options.html_requested().map(|html| html.as_ref()),
            Some("<button>Save</button>")
        );

        let cleared = element.clear_html();
        assert!(cleared.options.html_requested().is_none());
    }

    #[test]
    fn webview_options_can_disable_javascript() {
        let options = WebViewOptions::embedded_widget().javascript_disabled();
        assert!(options.javascript_disabled_requested());

        let element = webview_with_options(
            "static-docs",
            "https://example.com",
            WebViewOptions::embedded_widget().javascript_disabled_enabled(false),
        );
        assert!(!element.options.javascript_disabled_requested());

        let disabled = element.javascript_disabled_enabled(true);
        assert!(disabled.options.javascript_disabled_requested());
    }

    #[test]
    fn webview_options_can_override_general_autofill() {
        let options = WebViewOptions::embedded_widget().general_autofill_disabled();
        assert_eq!(options.general_autofill_requested(), Some(false));

        let element = webview_with_options(
            "account-form",
            "https://example.com",
            WebViewOptions::embedded_widget().general_autofill_enabled(true),
        );
        assert_eq!(element.options.general_autofill_requested(), Some(true));

        let disabled = element.general_autofill_enabled(false);
        assert_eq!(disabled.options.general_autofill_requested(), Some(false));
    }

    #[test]
    fn webview_options_can_request_background_color() {
        let transparent = WebViewOptions::embedded_widget().transparent_background();
        assert_eq!(
            transparent.background_color_requested(),
            Some(crate::Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0
            })
        );

        let element = webview_with_options(
            "native-card",
            "https://example.com",
            WebViewOptions::embedded_widget().background_color(rgb(0x112233)),
        );
        assert_eq!(
            element.options.background_color_requested(),
            Some(rgb(0x112233))
        );

        let transparent_element = element.transparent_background();
        assert_eq!(
            transparent_element
                .options
                .background_color_requested()
                .map(|color| color.a),
            Some(0.0)
        );
    }

    #[test]
    fn webview_options_can_control_new_window_requests() {
        let denied = WebViewOptions::embedded_widget().deny_new_windows();
        assert!(denied.on_new_window.is_some());

        let current = webview("docs", "https://example.com").open_new_windows_in_current_webview();
        assert!(current.options.on_new_window.is_some());

        let allowed = webview_with_options(
            "auth-popup",
            "https://example.com",
            WebViewOptions::auth_flow("auth").allow_new_windows(),
        );
        assert!(allowed.options.on_new_window.is_some());

        let custom = WebViewOptions::new().on_new_window(|url, _, _| {
            if url.as_ref().starts_with("https://trusted.example") {
                WebViewNewWindowPolicy::NavigateCurrent
            } else {
                WebViewNewWindowPolicy::Deny
            }
        });
        assert!(custom.on_new_window.is_some());
    }

    #[test]
    fn webview_options_can_control_downloads() {
        let denied = WebViewOptions::embedded_widget().deny_downloads();
        assert!(denied.on_download_started.is_some());

        let allowed = webview("downloads", "https://example.com").allow_downloads();
        assert!(allowed.options.on_download_started.is_some());

        let custom_destination = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            .join("kael-download.bin");
        let custom = WebViewOptions::new()
            .on_download_started(move |_, _, _, _| {
                WebViewDownloadPolicy::SaveTo(custom_destination.clone())
            })
            .on_download_completed(|event, _, _| {
                let WebViewDownloadCompleted { url, path, success } = event;
                let _ = (url, path, success);
            });
        assert!(custom.on_download_started.is_some());
        assert!(custom.on_download_completed.is_some());
    }

    #[test]
    fn webview_options_can_observe_document_title_changes() {
        let options =
            WebViewOptions::embedded_widget().on_document_title_changed(|title, _window, _cx| {
                let _ = title;
            });
        assert!(options.on_document_title_changed.is_some());

        let element =
            webview("docs", "https://example.com").on_document_title_changed(|title, _, _| {
                let _ = title;
            });
        assert!(element.options.on_document_title_changed.is_some());
    }

    #[test]
    fn webview_options_can_observe_page_load_events() {
        let options = WebViewOptions::embedded_widget().on_page_load(|event, url, _, _| {
            let _ = (event, url);
        });
        assert!(options.on_page_load.is_some());

        let element = webview("docs", "https://example.com").on_page_load(|event, url, _, _| {
            match event {
                WebViewPageLoadEvent::Started | WebViewPageLoadEvent::Finished => {}
            }
            let _ = url;
        });
        assert!(element.options.on_page_load.is_some());
    }

    #[test]
    fn webview_options_can_control_drag_drop_defaults() {
        let blocked = WebViewOptions::embedded_widget().block_drag_drop();
        assert!(blocked.drag_drop_handler_requested());

        let options = WebViewOptions::embedded_widget().on_drag_drop(|event, _, _| {
            match event {
                WebViewDragDropEvent::Enter { paths, position }
                | WebViewDragDropEvent::Drop { paths, position } => {
                    let _ = (paths, position);
                }
                WebViewDragDropEvent::Over { position } => {
                    let _ = position;
                }
                WebViewDragDropEvent::Leave => {}
            }
            WebViewDragDropPolicy::AllowBrowserDefault
        });
        assert!(options.drag_drop_handler_requested());

        let element = webview("drop-zone", "https://example.com").block_drag_drop();
        assert!(element.options.drag_drop_handler_requested());
    }

    #[test]
    fn webview_with_options_applies_reusable_options() {
        let element = webview_with_options(
            "checkout",
            "https://example.com",
            WebViewOptions::embedded_widget().user_agent("Kael-Test"),
        );

        assert_eq!(
            element
                .options
                .user_agent
                .as_ref()
                .map(|value| value.as_ref()),
            Some("Kael-Test")
        );
        assert_eq!(element.options.injected_css.len(), 1);
    }

    #[test]
    fn webview_file_url_encodes_local_paths() {
        let path = std::env::temp_dir().join("Kael Preview File.html");
        let url = webview_file_url(&path).unwrap();

        assert!(url.starts_with("file://"));
        assert!(url.contains("Kael%20Preview%20File.html"));
    }

    #[test]
    fn webview_file_with_options_applies_reusable_options() {
        let path = std::env::temp_dir().join("kael-preview.html");
        let element = webview_file_with_options(
            "local-preview",
            &path,
            WebViewOptions::embedded_widget().bridge_script(),
        )
        .unwrap();

        assert!(element.url.starts_with("file://"));
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
    }

    #[test]
    fn webview_html_url_embeds_document_as_base64_data_url() {
        let html = "<!doctype html><button>Hi</button>";
        let url = webview_html_url(html);
        let prefix = "data:text/html;charset=utf-8;base64,";

        assert!(url.starts_with(prefix));
        let encoded = url.as_ref().trim_start_matches(prefix);
        let decoded = BASE64.decode(encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), html);
    }

    #[test]
    fn webview_html_with_options_applies_reusable_options() {
        let element = webview_html_with_options(
            "inline-widget",
            "<!doctype html><p>Hello</p>",
            WebViewOptions::embedded_widget().bridge_script(),
        );

        assert!(element.url.is_empty());
        assert_eq!(
            element.options.html_requested().map(|html| html.as_ref()),
            Some("<!doctype html><p>Hello</p>")
        );
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
    }

    #[test]
    fn webview_bridge_message_round_trips_json_shape() {
        let message = WebViewBridgeMessage::with_payload(
            "select-file",
            serde_json::json!({ "accept": ["video/*"] }),
        )
        .with_id("request-1");

        let value = message.clone().into_value();
        assert_eq!(value["kind"], "select-file");
        assert_eq!(value["id"], "request-1");
        assert_eq!(value["payload"]["accept"][0], "video/*");
        assert_eq!(WebViewBridgeMessage::from_value(value), Some(message));
    }

    #[test]
    fn webview_find_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "find:result",
            serde_json::json!({
                "event": "find",
                "query": "Needle",
                "found": true,
                "matches": 4,
                "caseSensitive": true,
                "backwards": false,
                "wrap": true,
                "wholeWord": true,
                "searchInFrames": true,
                "selectionText": "Needle",
                "url": "https://example.test/docs"
            }),
        );

        let event = WebViewFindEvent::from_bridge_message(&message, "find:result").unwrap();
        assert_eq!(event.event.as_ref(), "find");
        assert_eq!(event.query.as_ref(), "Needle");
        assert!(event.found);
        assert_eq!(event.matches, 4);
        assert!(event.case_sensitive);
        assert!(!event.backwards);
        assert!(event.wrap);
        assert!(event.whole_word);
        assert!(event.search_in_frames);
        assert_eq!(event.selection_text.as_ref(), "Needle");
        assert_eq!(event.url.as_ref(), "https://example.test/docs");
        assert!(WebViewFindEvent::from_bridge_message(&message, "other").is_none());
        assert!(WebViewFindEvent::from_payload(serde_json::json!("not find")).is_none());
    }

    #[test]
    fn webview_media_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "media:event",
            serde_json::json!({
                "event": "timeupdate",
                "state": {
                    "index": 0,
                    "tagName": "video",
                    "id": "preview",
                    "src": "https://example.test/movie.mp4",
                    "paused": false,
                    "ended": false,
                    "muted": false,
                    "volume": 0.75,
                    "playbackRate": 1.5,
                    "currentTime": 12.25,
                    "duration": 90.0,
                    "readyState": 4,
                    "networkState": 1,
                    "seeking": false,
                    "fullscreen": true,
                    "pictureInPicture": true,
                    "buffered": [{ "start": 0.0, "end": 20.0 }],
                    "textTracks": [{
                        "index": 0,
                        "id": "en",
                        "kind": "captions",
                        "label": "English",
                        "language": "en",
                        "mode": "showing",
                        "activeCues": [{ "id": "cue-1", "startTime": 12.0, "endTime": 14.0, "text": "Caption line" }]
                    }]
                }
            }),
        );

        let event = WebViewMediaEvent::from_bridge_message(&message, "media:event").unwrap();

        assert_eq!(event.event.as_ref(), "timeupdate");
        assert_eq!(event.state.tag_name.as_ref(), "video");
        assert_eq!(event.state.id.as_ref().unwrap().as_ref(), "preview");
        assert_eq!(event.state.current_time, 12.25);
        assert_eq!(event.state.playback_rate, 1.5);
        assert!(event.state.fullscreen);
        assert!(event.state.picture_in_picture);
        assert_eq!(event.state.buffered[0].end, 20.0);
        assert_eq!(event.state.text_tracks[0].kind.as_ref(), "captions");
        assert_eq!(
            event.state.text_tracks[0].active_cues[0].text.as_ref(),
            "Caption line"
        );
        assert_eq!(
            WebViewMediaEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewMediaEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewMediaEvent::from_payload(serde_json::json!("not media")).is_none());
    }

    #[test]
    fn webview_context_menu_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "context:menu",
            serde_json::json!({
                "x": 42.0,
                "y": 18.5,
                "selectedText": "Selected copy",
                "linkHref": "https://example.test/docs",
                "imageSrc": "https://example.test/preview.png",
                "mediaSrc": "https://example.test/clip.mp4",
                "editable": true,
                "inputKind": "textarea"
            }),
        );

        let event = WebViewContextMenuEvent::from_bridge_message(&message, "context:menu")
            .expect("context menu event");

        assert_eq!(event.x, 42.0);
        assert_eq!(event.y, 18.5);
        assert_eq!(event.selected_text.as_ref(), "Selected copy");
        assert_eq!(
            event.link_href.as_ref().unwrap().as_ref(),
            "https://example.test/docs"
        );
        assert_eq!(
            event.image_src.as_ref().unwrap().as_ref(),
            "https://example.test/preview.png"
        );
        assert_eq!(
            event.media_src.as_ref().unwrap().as_ref(),
            "https://example.test/clip.mp4"
        );
        assert!(event.editable);
        assert_eq!(event.input_kind.as_ref().unwrap().as_ref(), "textarea");
        assert_eq!(
            WebViewContextMenuEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewContextMenuEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewContextMenuEvent::from_payload(serde_json::json!("not context")).is_none());
    }

    #[test]
    fn webview_pointer_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "pointer:event",
            serde_json::json!({
                "event": "pointermove",
                "x": 42.0,
                "y": 18.5,
                "buttons": 1,
                "pointerType": "mouse",
                "targetTag": "A",
                "linkHref": "https://example.test/docs",
                "imageSrc": "https://example.test/preview.png",
                "mediaSrc": "https://example.test/clip.mp4",
                "editable": true,
                "inputKind": "text"
            }),
        );

        let event = WebViewPointerEvent::from_bridge_message(&message, "pointer:event")
            .expect("pointer event");

        assert_eq!(event.event.as_ref(), "pointermove");
        assert_eq!(event.x, 42.0);
        assert_eq!(event.y, 18.5);
        assert_eq!(event.buttons, 1);
        assert_eq!(event.pointer_type.as_ref(), "mouse");
        assert_eq!(event.target_tag.as_ref().unwrap().as_ref(), "A");
        assert_eq!(
            event.link_href.as_ref().unwrap().as_ref(),
            "https://example.test/docs"
        );
        assert_eq!(
            event.image_src.as_ref().unwrap().as_ref(),
            "https://example.test/preview.png"
        );
        assert_eq!(
            event.media_src.as_ref().unwrap().as_ref(),
            "https://example.test/clip.mp4"
        );
        assert!(event.editable);
        assert_eq!(event.input_kind.as_ref().unwrap().as_ref(), "text");
        assert_eq!(
            WebViewPointerEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewPointerEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewPointerEvent::from_payload(serde_json::json!("not pointer")).is_none());
    }

    #[test]
    fn webview_form_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "form:event",
            serde_json::json!({
                "event": "submit",
                "formId": "checkout",
                "formName": "billing",
                "action": "https://example.test/pay",
                "method": "post",
                "target": "_self",
                "enctype": "application/x-www-form-urlencoded",
                "field": {
                    "name": "save",
                    "id": "save-button",
                    "tagName": "input",
                    "inputKind": "submit",
                    "value": "Save",
                    "checked": null,
                    "disabled": false,
                    "required": false
                },
                "fields": [
                    {
                        "name": "email",
                        "id": "email",
                        "tagName": "input",
                        "inputKind": "email",
                        "value": "person@example.test",
                        "checked": null,
                        "disabled": false,
                        "required": true
                    },
                    {
                        "name": "password",
                        "id": "password",
                        "tagName": "input",
                        "inputKind": "password",
                        "value": null,
                        "checked": null,
                        "disabled": false,
                        "required": true
                    },
                    {
                        "name": "topics",
                        "id": "topics",
                        "tagName": "select",
                        "inputKind": "select-multiple",
                        "value": ["rust", "desktop"],
                        "checked": null,
                        "disabled": false,
                        "required": false
                    }
                ],
                "defaultPrevented": true
            }),
        );

        let event =
            WebViewFormEvent::from_bridge_message(&message, "form:event").expect("form event");

        assert_eq!(event.event.as_ref(), "submit");
        assert_eq!(event.form_id.as_ref().unwrap().as_ref(), "checkout");
        assert_eq!(event.form_name.as_ref().unwrap().as_ref(), "billing");
        assert_eq!(
            event.action.as_ref().unwrap().as_ref(),
            "https://example.test/pay"
        );
        assert_eq!(event.method.as_ref(), "post");
        assert_eq!(event.target.as_ref().unwrap().as_ref(), "_self");
        assert_eq!(
            event.enctype.as_ref().unwrap().as_ref(),
            "application/x-www-form-urlencoded"
        );
        assert!(event.default_prevented);
        let field = event.field.as_ref().expect("submitter field");
        assert_eq!(field.name.as_ref().unwrap().as_ref(), "save");
        assert_eq!(field.input_kind.as_ref(), "submit");
        assert_eq!(field.value.as_ref().unwrap(), &serde_json::json!("Save"));
        assert_eq!(event.fields.len(), 3);
        assert_eq!(event.fields[0].name.as_ref().unwrap().as_ref(), "email");
        assert_eq!(event.fields[0].tag_name.as_ref(), "input");
        assert_eq!(event.fields[0].input_kind.as_ref(), "email");
        assert_eq!(
            event.fields[0].value.as_ref().unwrap(),
            &serde_json::json!("person@example.test")
        );
        assert!(event.fields[0].required);
        assert_eq!(event.fields[1].input_kind.as_ref(), "password");
        assert!(event.fields[1].value.is_none());
        assert_eq!(
            event.fields[2].value.as_ref().unwrap(),
            &serde_json::json!(["rust", "desktop"])
        );
        assert_eq!(
            WebViewFormEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewFormEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewFormEvent::from_payload(serde_json::json!("not form")).is_none());
    }

    #[test]
    fn webview_file_input_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "file:event",
            serde_json::json!({
                "event": "change",
                "inputName": "attachments",
                "inputId": "attachments-field",
                "accept": "image/*,.pdf",
                "multiple": true,
                "formId": "upload-form",
                "formName": "documents",
                "action": "https://example.test/upload",
                "method": "post",
                "files": [
                    {
                        "name": "design.pdf",
                        "size": 42_000,
                        "mimeType": "application/pdf",
                        "lastModified": 1_700_000_000_000u64
                    },
                    {
                        "name": "preview.png",
                        "size": 9_500,
                        "mimeType": "image/png",
                        "lastModified": null
                    }
                ]
            }),
        );

        let event = WebViewFileInputEvent::from_bridge_message(&message, "file:event")
            .expect("file input event");

        assert_eq!(event.event.as_ref(), "change");
        assert_eq!(event.input_name.as_ref().unwrap().as_ref(), "attachments");
        assert_eq!(
            event.input_id.as_ref().unwrap().as_ref(),
            "attachments-field"
        );
        assert_eq!(event.accept.as_ref().unwrap().as_ref(), "image/*,.pdf");
        assert!(event.multiple);
        assert_eq!(event.form_id.as_ref().unwrap().as_ref(), "upload-form");
        assert_eq!(event.form_name.as_ref().unwrap().as_ref(), "documents");
        assert_eq!(
            event.action.as_ref().unwrap().as_ref(),
            "https://example.test/upload"
        );
        assert_eq!(event.method.as_ref(), "post");
        assert_eq!(event.files.len(), 2);
        assert_eq!(event.files[0].name.as_ref(), "design.pdf");
        assert_eq!(event.files[0].size, 42_000);
        assert_eq!(
            event.files[0].mime_type.as_ref().unwrap().as_ref(),
            "application/pdf"
        );
        assert_eq!(event.files[0].last_modified, Some(1_700_000_000_000));
        assert_eq!(event.files[1].name.as_ref(), "preview.png");
        assert_eq!(event.files[1].last_modified, None);
        assert_eq!(
            WebViewFileInputEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewFileInputEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewFileInputEvent::from_payload(serde_json::json!("not file")).is_none());
    }

    #[test]
    fn webview_resource_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "resource:event",
            serde_json::json!({
                "event": "resource",
                "url": "https://example.test/app.js",
                "initiatorType": "script",
                "targetTag": null,
                "success": null,
                "startTime": 12.5,
                "duration": 48.25,
                "transferSize": 2048,
                "encodedBodySize": 1536,
                "decodedBodySize": 4096,
                "nextHopProtocol": "h2",
                "renderBlockingStatus": "non-blocking"
            }),
        );

        let event = WebViewResourceEvent::from_bridge_message(&message, "resource:event")
            .expect("resource event");

        assert_eq!(event.event.as_ref(), "resource");
        assert_eq!(event.url.as_ref(), "https://example.test/app.js");
        assert_eq!(event.initiator_type.as_ref(), "script");
        assert_eq!(event.target_tag, None);
        assert_eq!(event.success, None);
        assert_eq!(event.start_time, 12.5);
        assert_eq!(event.duration, 48.25);
        assert_eq!(event.transfer_size, 2048);
        assert_eq!(event.encoded_body_size, 1536);
        assert_eq!(event.decoded_body_size, 4096);
        assert_eq!(event.next_hop_protocol.as_ref().unwrap().as_ref(), "h2");
        assert_eq!(
            event.render_blocking_status.as_ref().unwrap().as_ref(),
            "non-blocking"
        );
        assert_eq!(
            WebViewResourceEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewResourceEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewResourceEvent::from_payload(serde_json::json!("not resource")).is_none());

        let failed_image = WebViewResourceEvent::from_payload(serde_json::json!({
            "event": "error",
            "url": "https://example.test/missing.png",
            "initiatorType": "img",
            "targetTag": "IMG",
            "success": false
        }))
        .expect("element resource event");
        assert_eq!(failed_image.event.as_ref(), "error");
        assert_eq!(failed_image.target_tag.as_ref().unwrap().as_ref(), "IMG");
        assert_eq!(failed_image.success, Some(false));
    }

    #[test]
    fn webview_network_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "network:event",
            serde_json::json!({
                "event": "fetch",
                "api": "fetch",
                "method": "POST",
                "url": "https://example.test/api/items",
                "status": 201,
                "statusText": "Created",
                "ok": true,
                "durationMs": 42.5,
                "errorName": null,
                "errorMessage": null,
                "responseType": null,
                "documentUrl": "https://example.test/app"
            }),
        );

        let event = WebViewNetworkEvent::from_bridge_message(&message, "network:event")
            .expect("network event");

        assert_eq!(event.event.as_ref(), "fetch");
        assert_eq!(event.api.as_ref(), "fetch");
        assert_eq!(event.method.as_ref(), "POST");
        assert_eq!(event.url.as_ref(), "https://example.test/api/items");
        assert_eq!(event.status, Some(201));
        assert_eq!(event.status_text.as_ref().unwrap().as_ref(), "Created");
        assert_eq!(event.ok, Some(true));
        assert_eq!(event.duration_ms, 42.5);
        assert!(event.error_name.is_none());
        assert!(event.error_message.is_none());
        assert!(event.response_type.is_none());
        assert_eq!(event.document_url.as_ref(), "https://example.test/app");
        assert_eq!(
            WebViewNetworkEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewNetworkEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewNetworkEvent::from_payload(serde_json::json!("not network")).is_none());

        let failed_fetch = WebViewNetworkEvent::from_payload(serde_json::json!({
            "event": "fetch-error",
            "api": "fetch",
            "method": "GET",
            "url": "https://example.test/offline",
            "status": null,
            "ok": null,
            "durationMs": 10.0,
            "errorName": "TypeError",
            "errorMessage": "Failed to fetch",
            "documentUrl": "https://example.test/app"
        }))
        .expect("failed fetch event");
        assert_eq!(failed_fetch.event.as_ref(), "fetch-error");
        assert_eq!(failed_fetch.status, None);
        assert_eq!(
            failed_fetch.error_name.as_ref().unwrap().as_ref(),
            "TypeError"
        );

        let xhr = WebViewNetworkEvent::from_payload(serde_json::json!({
            "event": "xhr",
            "api": "XMLHttpRequest",
            "method": "GET",
            "url": "https://example.test/api/items",
            "status": 404,
            "statusText": "Not Found",
            "ok": false,
            "durationMs": 7.5,
            "responseType": "json",
            "documentUrl": "https://example.test/app"
        }))
        .expect("xhr event");
        assert_eq!(xhr.api.as_ref(), "XMLHttpRequest");
        assert_eq!(xhr.ok, Some(false));
        assert_eq!(xhr.response_type.as_ref().unwrap().as_ref(), "json");
    }

    #[test]
    fn webview_dialog_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "dialog:event",
            serde_json::json!({
                "event": "confirm",
                "message": "Delete draft?",
                "defaultValue": null,
                "result": true,
                "url": "https://example.test/editor",
                "defaultPrevented": false
            }),
        );

        let event = WebViewDialogEvent::from_bridge_message(&message, "dialog:event")
            .expect("dialog event");

        assert_eq!(event.event.as_ref(), "confirm");
        assert_eq!(event.message.as_ref(), "Delete draft?");
        assert_eq!(event.default_value, None);
        assert_eq!(event.result.as_ref().unwrap(), &serde_json::json!(true));
        assert_eq!(event.url.as_ref(), "https://example.test/editor");
        assert!(!event.default_prevented);
        assert_eq!(
            WebViewDialogEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewDialogEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewDialogEvent::from_payload(serde_json::json!("not dialog")).is_none());

        let prompt = WebViewDialogEvent::from_payload(serde_json::json!({
            "event": "prompt",
            "message": "Name",
            "defaultValue": "Untitled",
            "result": "Project",
            "url": "https://example.test",
            "defaultPrevented": false
        }))
        .expect("prompt dialog");
        assert_eq!(prompt.default_value.as_ref().unwrap().as_ref(), "Untitled");
        assert_eq!(
            prompt.result.as_ref().unwrap(),
            &serde_json::json!("Project")
        );

        let before_unload = WebViewDialogEvent::from_payload(serde_json::json!({
            "event": "beforeunload",
            "message": "Unsaved changes",
            "result": null,
            "url": "https://example.test/editor",
            "defaultPrevented": true
        }))
        .expect("beforeunload dialog");
        assert_eq!(before_unload.event.as_ref(), "beforeunload");
        assert!(before_unload.default_prevented);
    }

    #[test]
    fn webview_clipboard_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "clipboard:event",
            serde_json::json!({
                "event": "paste",
                "types": ["text/plain", "text/html"],
                "text": "Hello",
                "html": "<strong>Hello</strong>",
                "targetEditable": true,
                "url": "https://example.test/editor",
                "defaultPrevented": false
            }),
        );

        let event = WebViewClipboardEvent::from_bridge_message(&message, "clipboard:event")
            .expect("clipboard event");

        assert_eq!(event.event.as_ref(), "paste");
        assert_eq!(event.types.len(), 2);
        assert_eq!(event.types[0].as_ref(), "text/plain");
        assert_eq!(event.types[1].as_ref(), "text/html");
        assert_eq!(event.text.as_ref().unwrap().as_ref(), "Hello");
        assert_eq!(
            event.html.as_ref().unwrap().as_ref(),
            "<strong>Hello</strong>"
        );
        assert!(event.target_editable);
        assert_eq!(event.url.as_ref(), "https://example.test/editor");
        assert!(!event.default_prevented);
        assert_eq!(
            WebViewClipboardEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewClipboardEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewClipboardEvent::from_payload(serde_json::json!("not clipboard")).is_none());

        let cut = WebViewClipboardEvent::from_payload(serde_json::json!({
            "event": "cut",
            "types": [],
            "text": null,
            "html": null,
            "targetEditable": true,
            "url": "https://example.test/editor",
            "defaultPrevented": true
        }))
        .expect("cut event");
        assert_eq!(cut.event.as_ref(), "cut");
        assert!(cut.text.is_none());
        assert!(cut.default_prevented);
    }

    #[test]
    fn webview_permission_request_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "permission:request",
            serde_json::json!({
                "permission": "media",
                "permissions": ["camera", "microphone"],
                "api": "mediaDevices.getUserMedia",
                "url": "https://example.test/call",
                "origin": "https://example.test",
                "userGesture": true,
                "details": {
                    "constraints": {
                        "video": true,
                        "audio": { "echoCancellation": true }
                    }
                }
            }),
        );

        let request = WebViewPermissionRequest::from_bridge_message(&message, "permission:request")
            .expect("permission request");

        assert_eq!(request.permission.as_ref(), "media");
        assert_eq!(request.permissions.len(), 2);
        assert_eq!(request.permissions[0].as_ref(), "camera");
        assert_eq!(request.permissions[1].as_ref(), "microphone");
        assert_eq!(request.api.as_ref(), "mediaDevices.getUserMedia");
        assert_eq!(request.url.as_ref(), "https://example.test/call");
        assert_eq!(request.origin.as_ref(), "https://example.test");
        assert!(request.user_gesture);
        assert_eq!(request.details["constraints"]["video"], true);
        assert_eq!(
            request.details["constraints"]["audio"]["echoCancellation"],
            true
        );
        assert_eq!(
            WebViewPermissionRequest::from_payload(message.payload.clone()),
            Some(request)
        );
        assert!(WebViewPermissionRequest::from_bridge_message(&message, "other:event").is_none());
        assert!(
            WebViewPermissionRequest::from_payload(serde_json::json!("not permission")).is_none()
        );
        assert_eq!(
            WebViewPermissionDecision::Default.as_bridge_str(),
            "default"
        );
        assert_eq!(WebViewPermissionDecision::Allow.as_bridge_str(), "allow");
        assert_eq!(WebViewPermissionDecision::Deny.as_bridge_str(), "deny");

        let display = WebViewPermissionRequest::from_payload(serde_json::json!({
            "permission": "display-capture",
            "permissions": ["display-capture", "system-audio"],
            "api": "mediaDevices.getDisplayMedia",
            "url": "https://example.test/call",
            "origin": "https://example.test",
            "userGesture": true,
            "details": {
                "constraints": {
                    "video": true,
                    "audio": true
                }
            }
        }))
        .expect("display capture request");
        assert_eq!(display.permission.as_ref(), "display-capture");
        assert_eq!(display.permissions[0].as_ref(), "display-capture");
        assert_eq!(display.permissions[1].as_ref(), "system-audio");
        assert_eq!(display.api.as_ref(), "mediaDevices.getDisplayMedia");
        assert_eq!(display.details["constraints"]["audio"], true);
    }

    #[test]
    fn webview_storage_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "storage:event",
            serde_json::json!({
                "event": "setItem",
                "area": "localStorage",
                "key": "theme",
                "oldValue": "light",
                "newValue": "dark",
                "length": 3,
                "url": "https://example.test/settings",
                "local": true
            }),
        );

        let event = WebViewStorageEvent::from_bridge_message(&message, "storage:event")
            .expect("storage event");

        assert_eq!(event.event.as_ref(), "setItem");
        assert_eq!(event.area.as_ref(), "localStorage");
        assert_eq!(event.key.as_ref().unwrap().as_ref(), "theme");
        assert_eq!(event.old_value.as_ref().unwrap().as_ref(), "light");
        assert_eq!(event.new_value.as_ref().unwrap().as_ref(), "dark");
        assert_eq!(event.length, 3);
        assert_eq!(event.url.as_ref(), "https://example.test/settings");
        assert!(event.local);
        assert_eq!(
            WebViewStorageEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewStorageEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewStorageEvent::from_payload(serde_json::json!("not storage")).is_none());

        let clear = WebViewStorageEvent::from_payload(serde_json::json!({
            "event": "clear",
            "area": "sessionStorage",
            "key": null,
            "oldValue": "<cleared>",
            "newValue": null,
            "length": 0,
            "url": "https://example.test/settings",
            "local": true
        }))
        .expect("clear event");
        assert_eq!(clear.event.as_ref(), "clear");
        assert_eq!(clear.area.as_ref(), "sessionStorage");
        assert!(clear.key.is_none());
        assert!(clear.new_value.is_none());
    }

    #[test]
    fn webview_favicon_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "favicon:changed",
            serde_json::json!({
                "urls": [
                    "https://example.test/favicon.ico",
                    "https://example.test/icon.svg"
                ]
            }),
        );

        let event = WebViewFaviconEvent::from_bridge_message(&message, "favicon:changed")
            .expect("favicon event");

        assert_eq!(
            event.urls,
            vec![
                SharedString::from("https://example.test/favicon.ico"),
                SharedString::from("https://example.test/icon.svg")
            ]
        );
        assert_eq!(
            WebViewFaviconEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewFaviconEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewFaviconEvent::from_payload(serde_json::json!("not favicon")).is_none());
    }

    #[test]
    fn webview_console_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "console:message",
            serde_json::json!({
                "level": "warn",
                "message": "Heads up {\"count\":2}",
                "args": ["Heads up", { "count": 2 }],
                "source": "https://example.test/app",
                "line": 12,
                "column": 34
            }),
        );

        let event = WebViewConsoleEvent::from_bridge_message(&message, "console:message")
            .expect("console event");

        assert_eq!(event.level.as_ref(), "warn");
        assert_eq!(event.message.as_ref(), "Heads up {\"count\":2}");
        assert_eq!(event.args[0], serde_json::json!("Heads up"));
        assert_eq!(event.args[1], serde_json::json!({ "count": 2 }));
        assert_eq!(
            event.source.as_ref().map(|source| source.as_ref()),
            Some("https://example.test/app")
        );
        assert_eq!(event.line, Some(12));
        assert_eq!(event.column, Some(34));
        assert_eq!(
            WebViewConsoleEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewConsoleEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewConsoleEvent::from_payload(serde_json::json!("not console")).is_none());
    }

    #[test]
    fn webview_keyboard_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "keyboard:event",
            serde_json::json!({
                "event": "keydown",
                "key": "k",
                "code": "KeyK",
                "location": 0,
                "repeat": true,
                "isComposing": false,
                "altKey": false,
                "ctrlKey": true,
                "metaKey": false,
                "shiftKey": true,
                "targetEditable": true,
                "inputType": "insertText",
                "data": "k",
                "defaultPrevented": false
            }),
        );

        let event = WebViewKeyboardEvent::from_bridge_message(&message, "keyboard:event")
            .expect("keyboard event");

        assert_eq!(event.event.as_ref(), "keydown");
        assert_eq!(event.key.as_ref().unwrap().as_ref(), "k");
        assert_eq!(event.code.as_ref().unwrap().as_ref(), "KeyK");
        assert!(event.repeat);
        assert!(event.ctrl_key);
        assert!(event.shift_key);
        assert!(event.target_editable);
        assert_eq!(event.input_type.as_ref().unwrap().as_ref(), "insertText");
        assert_eq!(event.data.as_ref().unwrap().as_ref(), "k");
        assert!(!event.default_prevented);
        assert_eq!(
            WebViewKeyboardEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewKeyboardEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewKeyboardEvent::from_payload(serde_json::json!("not keyboard")).is_none());
    }

    #[test]
    fn webview_location_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "location:changed",
            serde_json::json!({
                "url": "https://example.test/app/settings#profile",
                "title": "Settings",
                "readyState": "complete",
                "canGoBack": true,
                "canGoForward": false
            }),
        );

        let event = WebViewLocationEvent::from_bridge_message(&message, "location:changed")
            .expect("location event");

        assert_eq!(
            event.url.as_ref(),
            "https://example.test/app/settings#profile"
        );
        assert_eq!(event.title.as_ref(), "Settings");
        assert_eq!(event.ready_state.as_ref(), "complete");
        assert!(event.can_go_back);
        assert!(!event.can_go_forward);
        assert_eq!(
            WebViewLocationEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewLocationEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewLocationEvent::from_payload(serde_json::json!("not location")).is_none());
    }

    #[test]
    fn webview_lifecycle_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "lifecycle:event",
            serde_json::json!({
                "event": "visibilitychange",
                "visibilityState": "hidden",
                "hidden": true,
                "hasFocus": false,
                "fullscreen": true,
                "persisted": false
            }),
        );

        let event = WebViewLifecycleEvent::from_bridge_message(&message, "lifecycle:event")
            .expect("lifecycle event");

        assert_eq!(event.event.as_ref(), "visibilitychange");
        assert_eq!(event.visibility_state.as_ref(), "hidden");
        assert!(event.hidden);
        assert!(!event.has_focus);
        assert!(event.fullscreen);
        assert_eq!(event.persisted, Some(false));
        assert_eq!(
            WebViewLifecycleEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewLifecycleEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewLifecycleEvent::from_payload(serde_json::json!("not lifecycle")).is_none());
    }

    #[test]
    fn webview_scroll_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "scroll:event",
            serde_json::json!({
                "event": "scroll",
                "x": 12.5,
                "y": 250.0,
                "maxX": 100.0,
                "maxY": 1000.0,
                "viewportWidth": 800.0,
                "viewportHeight": 600.0,
                "scrollWidth": 900.0,
                "scrollHeight": 1600.0,
                "progressX": 0.125,
                "progressY": 0.25
            }),
        );

        let event = WebViewScrollEvent::from_bridge_message(&message, "scroll:event")
            .expect("scroll event");

        assert_eq!(event.event.as_ref(), "scroll");
        assert_eq!(event.x, 12.5);
        assert_eq!(event.y, 250.0);
        assert_eq!(event.max_x, 100.0);
        assert_eq!(event.max_y, 1000.0);
        assert_eq!(event.viewport_width, 800.0);
        assert_eq!(event.viewport_height, 600.0);
        assert_eq!(event.scroll_width, 900.0);
        assert_eq!(event.scroll_height, 1600.0);
        assert_eq!(event.progress_x, 0.125);
        assert_eq!(event.progress_y, 0.25);
        assert_eq!(
            WebViewScrollEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewScrollEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewScrollEvent::from_payload(serde_json::json!("not scroll")).is_none());
    }

    #[test]
    fn webview_selection_event_parses_bridge_payloads() {
        let message = WebViewBridgeMessage::with_payload(
            "selection:event",
            serde_json::json!({
                "event": "selectionchange",
                "selectedText": "Selected copy",
                "selectedHtml": "<strong>Selected copy</strong>",
                "collapsed": false,
                "editable": true,
                "inputKind": "contenteditable"
            }),
        );

        let event = WebViewSelectionEvent::from_bridge_message(&message, "selection:event")
            .expect("selection event");

        assert_eq!(event.event.as_ref(), "selectionchange");
        assert_eq!(event.selected_text.as_ref(), "Selected copy");
        assert_eq!(
            event.selected_html.as_ref(),
            "<strong>Selected copy</strong>"
        );
        assert!(!event.collapsed);
        assert!(event.editable);
        assert_eq!(
            event.input_kind.as_ref().unwrap().as_ref(),
            "contenteditable"
        );
        assert_eq!(
            WebViewSelectionEvent::from_payload(message.payload.clone()),
            Some(event)
        );
        assert!(WebViewSelectionEvent::from_bridge_message(&message, "other:event").is_none());
        assert!(WebViewSelectionEvent::from_payload(serde_json::json!("not selection")).is_none());
    }

    #[test]
    fn webview_bridge_message_builds_response_and_error_envelopes() {
        let request = WebViewBridgeMessage::with_payload(
            "pick-video",
            serde_json::json!({ "multiple": false }),
        )
        .with_id("request-42");

        let response = WebViewBridgeMessage::response_to(
            &request,
            serde_json::json!({ "path": "/tmp/a.mp4" }),
        );
        assert_eq!(response.kind.as_ref(), "pick-video:response");
        assert_eq!(
            response.id.as_ref().map(|id| id.as_ref()),
            Some("request-42")
        );
        assert_eq!(response.payload["path"], "/tmp/a.mp4");

        let error = WebViewBridgeMessage::error_to(&request, "cancelled");
        assert_eq!(error.kind.as_ref(), "pick-video:error");
        assert_eq!(error.id.as_ref().map(|id| id.as_ref()), Some("request-42"));
        assert_eq!(error.payload["message"], "cancelled");
    }

    #[test]
    fn webview_bridge_script_exposes_window_kael_helpers() {
        let script = webview_bridge_script();

        assert!(script.contains("window.kael"));
        assert!(script.contains("invoke(kind, payload, options = {})"));
        assert!(script.contains("post(kind, payload, id)"));
        assert!(script.contains("onMessage(handler)"));
        assert!(script.contains("pending.set(id"));
        assert!(script.contains(":response"));
    }

    #[test]
    fn webview_options_can_inject_bridge_script() {
        let options = WebViewOptions::embedded_widget().bridge_script();

        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
    }

    #[test]
    fn webview_options_can_inject_navigation_state_bridge() {
        let options = WebViewOptions::embedded_widget().navigation_state_bridge();

        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("__kaelNavigationState"))
        );

        let element = webview("docs", "https://example.com").navigation_state_bridge();
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("canGoForward"))
        );
    }

    #[test]
    fn webview_options_can_inject_find_result_bridge() {
        let options = WebViewOptions::embedded_widget().find_result_bridge("find:result");

        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("window.kael"))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"find:result\""))
        );
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("__kaelFindResultBridge"))
        );

        let element = webview("docs", "https://example.com").find_result_bridge("tab:find");
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:find\""))
        );
    }

    #[test]
    fn webview_options_can_handle_typed_find_result_events() {
        let options =
            WebViewOptions::embedded_widget().on_find_result("find:result", |event, _, _| {
                let _ = event;
            });

        assert!(options.on_message.is_some());
        assert!(
            options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"find:result\""))
        );

        let element =
            webview("docs", "https://example.com").on_find_result("tab:find", |event, _, _| {
                let _ = event;
            });
        assert!(element.options.on_message.is_some());
        assert!(
            element
                .options
                .injected_javascript
                .iter()
                .any(|script| script.contains("\"tab:find\""))
        );
    }

    #[test]
    fn webview_bridge_handler_preserves_existing_raw_handler() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let raw_events = events.clone();
        let bridge_events = events.clone();
        let options = WebViewOptions::new()
            .on_message(move |_, _, _| {
                raw_events.borrow_mut().push("raw");
            })
            .on_bridge_message(move |_, _, _| {
                bridge_events.borrow_mut().push("bridge");
            });

        assert!(options.on_message.is_some());
    }

    #[test]
    fn navigation_scheme_helper_is_case_insensitive() {
        assert!(url_scheme_is_allowed(
            "HTTPS://example.com",
            &["https".into()]
        ));
        assert!(url_scheme_is_allowed(
            "data:text/plain,hello",
            &["data".into()]
        ));
        assert!(!url_scheme_is_allowed(
            "file:///etc/passwd",
            &["https".into()]
        ));
        assert!(!url_scheme_is_allowed("relative/path", &["https".into()]));
    }
}
