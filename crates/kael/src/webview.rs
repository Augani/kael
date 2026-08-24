use crate::{
    App, AsyncWindowContext, Bounds, GlobalElementId, Pixels, Rgba, SharedString, Window,
    platform_caps::SupportLevel,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
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

/// Permission category requested by the native embedded-browser engine.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum WebViewPermissionKind {
    /// Microphone capture.
    Microphone,
    /// Camera capture.
    Camera,
    /// Device location.
    Geolocation,
    /// Web notifications.
    Notifications,
    /// Reading clipboard contents.
    ClipboardRead,
    /// Screen or window capture.
    DisplayCapture,
    /// System-exclusive MIDI access.
    Midi,
    /// Motion or environmental sensors.
    Sensors,
    /// Protected-media key-system access.
    MediaKeySystemAccess,
    /// Enumeration or use of locally installed fonts.
    LocalFonts,
    /// Browser window-placement or window-management access.
    WindowManagement,
    /// Locking the pointer to the WebView.
    PointerLock,
    /// Multiple automatic downloads.
    AutomaticDownloads,
    /// Browser file-system read or write access.
    FileSystemAccess,
    /// Media autoplay.
    Autoplay,
    /// A backend permission category not represented above.
    Other,
}

/// Sandbox strength for an iframe-backed WebView in a Kael browser build.
///
/// Native WebViews do not use this policy. Browser builds default to
/// [`Self::Strict`] so same-origin hosted content cannot reach the Kael host
/// page's DOM merely because it shares an origin.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum BrowserWebViewSandbox {
    /// Run scripts and common embedded workflows in an opaque iframe origin.
    /// Cookies, origin storage, popups, top-level navigation, and direct host
    /// DOM access are unavailable.
    #[default]
    Strict,
    /// Preserve the iframe's real origin and storage.
    ///
    /// Only use this for trusted content. Combining scripts and same-origin
    /// access lets a same-origin document escape iframe sandbox restrictions.
    TrustedSameOrigin,
    /// Do not set an iframe `sandbox` attribute.
    ///
    /// This most closely resembles a native WebView, but same-origin content
    /// can directly access and modify the Kael host page.
    Unrestricted,
}

/// Loading strategy for an iframe-backed browser WebView.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum BrowserWebViewLoading {
    /// Load the hosted surface immediately.
    #[default]
    Eager,
    /// Let the browser defer loading until the surface approaches the viewport.
    Lazy,
}

/// Concrete embedded-browser backend selected for the current build/runtime.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WebViewBackend {
    /// Browser-runtime iframe composition island.
    BrowserIframe,
    /// Native macOS WKWebView child.
    MacOsWkWebView,
    /// Native Windows WebView2 child.
    WindowsWebView2,
    /// Native Linux WebKitGTK child hosted by X11 or XWayland.
    LinuxX11WebKitGtk,
    /// Native Linux WebKitGTK 6 child hosted in Kael's GTK4/GSK X11 window.
    LinuxX11WebKitGtk6,
    /// Native Linux WebKitGTK 6 child hosted in Kael's GTK4/GSK Wayland window.
    LinuxWaylandWebKitGtk6,
    /// A raw native Wayland build without the GTK4/WebKitGTK 6 host feature.
    LinuxWaylandUnavailable,
    /// Headless Linux sessions cannot host an interactive WebView.
    LinuxHeadlessUnavailable,
    /// WebView support was not enabled for this native build.
    Disabled,
    /// The target has no Kael WebView backend.
    UnsupportedPlatform,
}

/// One operation in Kael's portable WebView contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WebViewCapability {
    /// Bounds, visibility, opacity, clipping, and composition with retained UI.
    Composition,
    /// Moving focus into the hosted document and back to Kael.
    Focus,
    /// URL and inline-HTML navigation.
    Navigation,
    /// Allow/deny navigation policy callbacks.
    NavigationPolicy,
    /// Started/finished page-load and document-title events.
    LoadState,
    /// Back and forward history commands and state queries.
    History,
    /// Reloading and stopping an in-flight load.
    ReloadAndStop,
    /// Programmatic zoom.
    Zoom,
    /// Finding and selecting document text.
    Find,
    /// Printing the hosted document.
    Print,
    /// Download policy, destination, and completion callbacks.
    Downloads,
    /// Authenticated host/document message passing.
    Ipc,
    /// Serving app-owned custom URL protocols inside the WebView.
    CustomProtocols,
    /// Programmatic developer-tool control and state.
    DeveloperTools,
    /// Cookie read, set, and delete operations.
    Cookies,
    /// Initial and controller-directed custom navigation headers.
    RequestHeaders,
    /// Named persistent profile isolation and profile data clearing.
    ProfileIsolation,
    /// Per-WebView user-agent overrides.
    UserAgent,
    /// JavaScript injection, evaluation, and serialized results.
    JavaScript,
    /// Native browser permission decisions with origin/frame context.
    NativePermissions,
    /// Host-side file drag/drop interception.
    DragDrop,
}

const WEBVIEW_CAPABILITIES: [WebViewCapability; 21] = [
    WebViewCapability::Composition,
    WebViewCapability::Focus,
    WebViewCapability::Navigation,
    WebViewCapability::NavigationPolicy,
    WebViewCapability::LoadState,
    WebViewCapability::History,
    WebViewCapability::ReloadAndStop,
    WebViewCapability::Zoom,
    WebViewCapability::Find,
    WebViewCapability::Print,
    WebViewCapability::Downloads,
    WebViewCapability::Ipc,
    WebViewCapability::CustomProtocols,
    WebViewCapability::DeveloperTools,
    WebViewCapability::Cookies,
    WebViewCapability::RequestHeaders,
    WebViewCapability::ProfileIsolation,
    WebViewCapability::UserAgent,
    WebViewCapability::JavaScript,
    WebViewCapability::NativePermissions,
    WebViewCapability::DragDrop,
];

/// Support entry for one [`WebViewCapability`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewCapabilityEntry {
    /// Operation being reported.
    pub capability: WebViewCapability,
    /// Full, partial, unsupported, or disabled support.
    pub support: SupportLevel,
    /// Exact backend limitation or guarantee.
    pub note: String,
}

impl WebViewCapabilityEntry {
    /// Whether this operation has a usable implementation.
    pub fn is_available(&self) -> bool {
        self.support.is_available()
    }
}

/// Operation-level WebView support for one concrete backend.
///
/// This is deliberately more precise than
/// `CapabilityReport::support_for(PlatformFeature::WebView)`: applications can
/// gate a download destination picker, profile switcher, devtools button, or
/// cross-origin scripting workflow without guessing from the operating system.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebViewCapabilityReport {
    /// Selected embedded-browser backend.
    pub backend: WebViewBackend,
    /// Aggregate availability of the WebView host itself.
    pub support: SupportLevel,
    /// Stable operation-level entries.
    pub capabilities: Vec<WebViewCapabilityEntry>,
}

impl WebViewCapabilityReport {
    /// Inspect the WebView backend selected for the current target/runtime.
    pub fn current() -> Self {
        Self::for_backend(current_webview_backend())
    }

    /// Build a deterministic report for a known backend.
    ///
    /// This is also useful for release tooling that renders a cross-platform
    /// matrix without launching every operating system.
    pub fn for_backend(backend: WebViewBackend) -> Self {
        let support = match backend {
            WebViewBackend::Disabled => SupportLevel::Disabled,
            WebViewBackend::LinuxWaylandUnavailable
            | WebViewBackend::LinuxHeadlessUnavailable
            | WebViewBackend::UnsupportedPlatform => SupportLevel::Unsupported,
            WebViewBackend::BrowserIframe
            | WebViewBackend::MacOsWkWebView
            | WebViewBackend::WindowsWebView2
            | WebViewBackend::LinuxX11WebKitGtk
            | WebViewBackend::LinuxX11WebKitGtk6
            | WebViewBackend::LinuxWaylandWebKitGtk6 => SupportLevel::Partial,
        };
        let capabilities = WEBVIEW_CAPABILITIES
            .into_iter()
            .map(|capability| {
                let (support, note) = webview_capability_for_backend(backend, capability);
                WebViewCapabilityEntry {
                    capability,
                    support,
                    note: note.to_owned(),
                }
            })
            .collect();
        Self {
            backend,
            support,
            capabilities,
        }
    }

    /// Return the report entry for one operation.
    pub fn entry(&self, capability: WebViewCapability) -> Option<&WebViewCapabilityEntry> {
        self.capabilities
            .iter()
            .find(|entry| entry.capability == capability)
    }

    /// Return the support level for one operation.
    pub fn support_for(&self, capability: WebViewCapability) -> SupportLevel {
        self.entry(capability)
            .map_or(SupportLevel::Unsupported, |entry| entry.support)
    }

    /// Whether one operation has a usable full or partial implementation.
    pub fn is_available(&self, capability: WebViewCapability) -> bool {
        self.support_for(capability).is_available()
    }

    /// Operations with no usable implementation on this backend.
    pub fn unavailable(&self) -> Vec<&WebViewCapabilityEntry> {
        self.capabilities
            .iter()
            .filter(|entry| !entry.is_available())
            .collect()
    }

    /// Compact diagnostic summary.
    pub fn to_text(&self) -> String {
        let full = self
            .capabilities
            .iter()
            .filter(|entry| entry.support == SupportLevel::Full)
            .count();
        let partial = self
            .capabilities
            .iter()
            .filter(|entry| entry.support == SupportLevel::Partial)
            .count();
        format!(
            "WebView backend {:?}: {} full, {} partial, {} unavailable operations",
            self.backend,
            full,
            partial,
            self.capabilities.len().saturating_sub(full + partial)
        )
    }
}

fn current_webview_backend() -> WebViewBackend {
    #[cfg(all(target_arch = "wasm32", feature = "browser"))]
    {
        return WebViewBackend::BrowserIframe;
    }
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(feature = "webview"),
        not(feature = "webview-wayland-gtk4"),
        not(any())
    ))]
    {
        return WebViewBackend::Disabled;
    }
    #[cfg(all(not(target_arch = "wasm32"), feature = "webview", target_os = "macos"))]
    {
        return WebViewBackend::MacOsWkWebView;
    }
    #[cfg(all(
        not(target_arch = "wasm32"),
        feature = "webview",
        target_os = "windows"
    ))]
    {
        return WebViewBackend::WindowsWebView2;
    }
    #[cfg(all(
        not(target_arch = "wasm32"),
        any(feature = "webview", feature = "webview-wayland-gtk4", any()),
        any(target_os = "linux", target_os = "freebsd")
    ))]
    {
        return match crate::platform::guess_compositor() {
            #[cfg(feature = "webview-wayland-gtk4")]
            "X11" => WebViewBackend::LinuxX11WebKitGtk6,
            #[cfg(all(any(), not(feature = "webview-wayland-gtk4")))]
            "X11" => WebViewBackend::LinuxX11WebKitGtk,
            #[cfg(not(any(feature = "webview", feature = "webview-wayland-gtk4", any())))]
            "X11" => WebViewBackend::Disabled,
            #[cfg(feature = "webview-wayland-gtk4")]
            "Wayland" => WebViewBackend::LinuxWaylandWebKitGtk6,
            #[cfg(not(feature = "webview-wayland-gtk4"))]
            "Wayland" => WebViewBackend::LinuxWaylandUnavailable,
            "Headless" => WebViewBackend::LinuxHeadlessUnavailable,
            _ => WebViewBackend::UnsupportedPlatform,
        };
    }
    #[allow(unreachable_code)]
    WebViewBackend::UnsupportedPlatform
}

fn webview_capability_for_backend(
    backend: WebViewBackend,
    capability: WebViewCapability,
) -> (SupportLevel, &'static str) {
    use SupportLevel::{Disabled, Full, Partial, Unsupported};
    use WebViewBackend::{
        BrowserIframe, Disabled as DisabledBackend, LinuxHeadlessUnavailable,
        LinuxWaylandUnavailable, LinuxWaylandWebKitGtk6, LinuxX11WebKitGtk, LinuxX11WebKitGtk6,
        MacOsWkWebView, UnsupportedPlatform, WindowsWebView2,
    };
    use WebViewCapability::*;

    match backend {
        DisabledBackend => (
            Disabled,
            "enable the portable `webview` feature (or a Linux compatibility host feature) for this build",
        ),
        LinuxWaylandUnavailable => (
            Unsupported,
            "this Wayland build did not enable the GTK4/GSK + WebKitGTK 6 host; enable `webview`",
        ),
        LinuxHeadlessUnavailable => (
            Unsupported,
            "interactive WebViews require a graphical Wayland or X11 display",
        ),
        UnsupportedPlatform => (Unsupported, "Kael has no WebView backend for this target"),
        BrowserIframe => match capability {
            Composition => (
                Partial,
                "retained rectangular iframe bounds, clipping, visibility, opacity, and z-order are supported; non-translation transforms are hidden",
            ),
            Focus => (Full, "focus moves between the iframe and Kael canvas"),
            Navigation => (
                Partial,
                "host-directed HTTP(S), data, blob, about, and inline navigation works; browser policy owns self-navigation and disallows native file URLs",
            ),
            NavigationPolicy => (
                Partial,
                "Kael enforces allowed origins and on_navigate for host-directed loads; arbitrary cross-origin self-navigation cannot be intercepted by the parent",
            ),
            LoadState => (
                Partial,
                "host loads emit started/finished and accessible documents expose titles; cross-origin dynamic title and resource state are opaque",
            ),
            History => (
                Partial,
                "back/forward works where the browser exposes frame history; cross-origin state queries remain opaque",
            ),
            ReloadAndStop => (
                Partial,
                "reload is frame-owned; stop requires script access and therefore cannot control arbitrary cross-origin frames",
            ),
            Zoom => (
                Partial,
                "same-origin/inline documents use CSS zoom; cross-origin iframe zoom is not exposed",
            ),
            Find => (
                Partial,
                "same-origin/inline document text can be selected and counted; cross-origin frames cannot be searched",
            ),
            Print => (
                Partial,
                "accessible hosted documents can open the browser print dialog; silent dispatch and app-owned print settings are unavailable",
            ),
            Downloads => (
                Partial,
                "sandboxed BrowserWebViewPolicy/download presets permit user-activated downloads; an unrestricted iframe cannot be denied by Kael, and destination paths plus reliable completion callbacks are browser-owned",
            ),
            Ipc => (
                Partial,
                "inline and parent-accessible same-origin documents use the authenticated bridge; cross-origin frames only receive exact-origin host postMessage",
            ),
            CustomProtocols => (
                Unsupported,
                "web pages cannot register arbitrary URL schemes for iframe navigation; use HTTP(S), blob, data, or app-owned byte loading",
            ),
            DeveloperTools => (
                Unsupported,
                "developer tools and their open state are controlled by the user's browser",
            ),
            Cookies => (
                Partial,
                "accessible document cookies are available; HttpOnly and other-origin cookies remain browser-protected",
            ),
            RequestHeaders => (
                Unsupported,
                "iframe navigation cannot attach application-selected request headers",
            ),
            ProfileIsolation => (
                Unsupported,
                "iframe storage and browsing data belong to the browser origin/profile",
            ),
            UserAgent => (
                Unsupported,
                "web applications cannot override an individual iframe user agent",
            ),
            JavaScript => (
                Partial,
                "inline and same-origin JavaScript/result evaluation works; cross-origin frame scripting is blocked",
            ),
            NativePermissions => (
                Partial,
                "iframe Permission Policy is configurable; browser prompts and decisions remain user-agent owned",
            ),
            DragDrop => (
                Unsupported,
                "host-side WebView drag/drop interception is not exposed; the hosted page retains its normal HTML drag/drop behavior",
            ),
        },
        MacOsWkWebView
        | WindowsWebView2
        | LinuxX11WebKitGtk
        | LinuxX11WebKitGtk6
        | LinuxWaylandWebKitGtk6 => match capability {
            Composition => (
                Partial,
                "native rectangular child composition supports bounds, clipping, visibility, and opacity; non-translation transforms are hidden and native islands remain above GPU content",
            ),
            Focus => (
                Full,
                "focus changes preserve the live document, history, and profile",
            ),
            Navigation => (
                Full,
                "URL, custom-header, and inline-HTML navigation are supported",
            ),
            NavigationPolicy => (
                Full,
                "main navigation and new-window policy callbacks are enforced",
            ),
            LoadState => (
                Full,
                "started/finished load and document-title callbacks are supported",
            ),
            History => (
                Full,
                "back/forward commands and portable state queries are supported",
            ),
            ReloadAndStop => (
                Full,
                "reload and standards-based stop-loading are supported",
            ),
            Zoom => (Full, "programmatic zoom factor is supported"),
            Find => (
                Partial,
                "portable DOM find and match counting work in the main document but do not include inaccessible cross-origin frames",
            ),
            Print => (Full, "the native embedded engine print path is supported"),
            Downloads => (
                Full,
                "allow/deny/save-to and completion callbacks are supported",
            ),
            Ipc => (
                Full,
                "main-frame, nonce-authenticated structured IPC is supported",
            ),
            CustomProtocols => (
                Full,
                "registered app-owned protocol routes are served through the native engine with status, MIME type, headers, and bounded body bytes",
            ),
            DeveloperTools => (
                Partial,
                "availability varies by engine/build; WKWebView uses Safari, WebView2 cannot close/query tools, and Wry release builds require `devtools`",
            ),
            Cookies => (
                Full,
                "profile cookie read/set/delete operations are supported",
            ),
            RequestHeaders => (
                Full,
                "initial and controller-directed navigation headers are supported",
            ),
            ProfileIsolation if backend == MacOsWkWebView => (
                Partial,
                "named WKWebsiteDataStore profiles require macOS 14+; older systems share the persistent default store",
            ),
            ProfileIsolation => (Full, "named persistent and incognito profiles are isolated"),
            UserAgent => (Full, "per-WebView user-agent overrides are supported"),
            JavaScript => (
                Full,
                "initialization scripts, evaluation, and serialized results are supported",
            ),
            NativePermissions if backend == WindowsWebView2 => (
                Partial,
                "requesting origin and user-gesture state are exposed, but frame identity is unavailable",
            ),
            NativePermissions
                if matches!(
                    backend,
                    LinuxX11WebKitGtk | LinuxX11WebKitGtk6 | LinuxWaylandWebKitGtk6
                ) =>
            {
                (
                    Partial,
                    "WebKitGTK exposes permission kind while Kael labels the current top-level origin as an approximation",
                )
            }
            NativePermissions => (
                Partial,
                "WKWebView exposes requesting origin/frame context for camera and microphone; other permission families remain engine-owned",
            ),
            DragDrop if backend == WindowsWebView2 => (
                Partial,
                "enabling host interception replaces WebView2 HTML drag/drop and cannot restore browser default handling per event",
            ),
            DragDrop if matches!(backend, LinuxX11WebKitGtk6 | LinuxWaylandWebKitGtk6) => (
                Full,
                "GTK4 file drag/drop callbacks can allow WebKit defaults or claim the operation per event",
            ),
            DragDrop => (
                Full,
                "native file drag/drop events and default-blocking policy are supported",
            ),
        },
    }
}

/// Security and capability policy for iframe-backed WebViews in browser builds.
///
/// The policy has no effect on native WKWebView, WebView2, or WebKitGTK hosts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserWebViewPolicy {
    /// Iframe sandbox strength.
    pub sandbox: BrowserWebViewSandbox,
    /// Serialized origins accepted for initial and controller-directed iframe
    /// navigations. An empty list accepts any supported WebView URL origin.
    /// Hosted documents can self-navigate unless they cooperate with Kael or
    /// are constrained by an application Content Security Policy.
    pub allowed_origins: Vec<SharedString>,
    /// Permission Policy capabilities delegated to the hosted frame.
    pub permissions: Vec<WebViewPermissionKind>,
    /// Referrer policy assigned to the iframe.
    pub referrer_policy: SharedString,
    /// Ask supporting browsers to load the iframe without ambient credentials.
    pub credentialless: bool,
    /// Browser loading strategy.
    pub loading: BrowserWebViewLoading,
    /// Permit user-activated downloads from sandboxed hosted frames.
    ///
    /// This adds the iframe `allow-downloads` sandbox token. It does not allow
    /// silent downloads, choose a destination, or bypass browser policy. It
    /// only affects [`BrowserWebViewSandbox::Strict`] and
    /// [`BrowserWebViewSandbox::TrustedSameOrigin`]; an unrestricted iframe
    /// intentionally has no sandbox for Kael to tighten.
    pub downloads: bool,
    /// Treat failure to install Kael's bridge as an integration error.
    /// Automatic bridge installation is limited to inline and parent-accessible
    /// same-origin documents in the 0.4 browser backend.
    pub cooperative_bridge_required: bool,
}

impl Default for BrowserWebViewPolicy {
    fn default() -> Self {
        Self {
            sandbox: BrowserWebViewSandbox::Strict,
            allowed_origins: Vec::new(),
            permissions: Vec::new(),
            referrer_policy: "no-referrer".into(),
            credentialless: false,
            loading: BrowserWebViewLoading::Eager,
            downloads: false,
            cooperative_bridge_required: false,
        }
    }
}

impl BrowserWebViewPolicy {
    /// Start with Kael's restrictive browser policy.
    pub fn strict() -> Self {
        Self::default()
    }

    /// Preserve origin storage for trusted hosted content.
    pub fn trusted_same_origin(mut self) -> Self {
        self.sandbox = BrowserWebViewSandbox::TrustedSameOrigin;
        self
    }

    /// Remove iframe sandboxing for app-controlled content.
    pub fn unrestricted(mut self) -> Self {
        self.sandbox = BrowserWebViewSandbox::Unrestricted;
        self
    }

    /// Restrict host-directed navigations and exact-origin bridge messages to
    /// these origins. Strict sandbox messages use an opaque origin and are
    /// authenticated by their frame source plus the per-host nonce instead.
    pub fn allowed_origins(
        mut self,
        origins: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.allowed_origins = origins.into_iter().map(Into::into).collect();
        self
    }

    /// Delegate a browser Permission Policy capability to the hosted frame.
    pub fn permission(mut self, permission: WebViewPermissionKind) -> Self {
        if !self.permissions.contains(&permission) {
            self.permissions.push(permission);
        }
        self
    }

    /// Set the iframe referrer policy.
    pub fn referrer_policy(mut self, policy: impl Into<SharedString>) -> Self {
        self.referrer_policy = policy.into();
        self
    }

    /// Enable or disable credentialless iframe loading where supported.
    pub fn credentialless(mut self, enabled: bool) -> Self {
        self.credentialless = enabled;
        self
    }

    /// Select eager or lazy iframe loading.
    pub fn loading(mut self, loading: BrowserWebViewLoading) -> Self {
        self.loading = loading;
        self
    }

    /// Permit or deny user-activated downloads in sandboxed hosted frames.
    ///
    /// This has no effect with [`BrowserWebViewSandbox::Unrestricted`], whose
    /// unsandboxed content remains subject to the browser's own policy.
    pub fn downloads(mut self, enabled: bool) -> Self {
        self.downloads = enabled;
        self
    }

    /// Require Kael to install its authenticated message bridge in the hosted
    /// document, logging an integration error when browser origin rules block it.
    pub fn require_cooperative_bridge(mut self, required: bool) -> Self {
        self.cooperative_bridge_required = required;
        self
    }
}

/// How accurately a native WebView backend identified the permission origin.
///
/// Permission engines do not expose the same context on every platform. A
/// policy that grants sensitive capabilities should normally require
/// [`Self::RequestingFrame`] rather than treating a top-level approximation as
/// the origin that actually initiated the request.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum WebViewPermissionOriginSource {
    /// The engine reported the origin of the frame that requested permission.
    RequestingFrame,
    /// The backend only exposed the current top-level document origin.
    TopLevelDocument,
    /// The backend did not expose a usable origin.
    #[default]
    Unavailable,
}

/// Frame context attached to a native WebView permission request.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum WebViewPermissionFrame {
    /// The top-level document initiated the request.
    Main,
    /// A child frame initiated the request.
    Subframe,
    /// The native engine did not expose frame identity.
    #[default]
    Unknown,
}

/// Security context supplied to a native WebView permission policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebViewNativePermissionRequest {
    /// Permission category requested by the embedded browser engine.
    pub kind: WebViewPermissionKind,
    /// Serialized origin when the backend exposes one.
    pub origin: Option<SharedString>,
    /// Whether `origin` names the requesting frame or only the top-level page.
    pub origin_source: WebViewPermissionOriginSource,
    /// Main-frame, subframe, or unknown frame context.
    pub frame: WebViewPermissionFrame,
    /// Whether the engine associated the request with a user gesture.
    pub user_gesture: Option<bool>,
}

impl WebViewNativePermissionRequest {
    /// Construct a conservative request when an engine exposes only the kind.
    pub fn kind_only(kind: WebViewPermissionKind) -> Self {
        Self {
            kind,
            origin: None,
            origin_source: WebViewPermissionOriginSource::Unavailable,
            frame: WebViewPermissionFrame::Unknown,
            user_gesture: None,
        }
    }

    /// Whether the origin belongs to the frame that actually requested access.
    pub fn has_exact_origin(&self) -> bool {
        self.origin.is_some()
            && self.origin_source == WebViewPermissionOriginSource::RequestingFrame
    }

    /// Whether the engine positively identified the main frame.
    pub fn is_main_frame(&self) -> bool {
        self.frame == WebViewPermissionFrame::Main
    }

    #[cfg(any(
        test,
        target_os = "macos",
        all(feature = "webview", target_os = "windows")
    ))]
    pub(crate) fn with_requesting_origin(
        kind: WebViewPermissionKind,
        origin: Option<SharedString>,
        frame: WebViewPermissionFrame,
        user_gesture: Option<bool>,
    ) -> Self {
        let origin_source = if origin.is_some() {
            WebViewPermissionOriginSource::RequestingFrame
        } else {
            WebViewPermissionOriginSource::Unavailable
        };
        Self {
            kind,
            origin,
            origin_source,
            frame,
            user_gesture,
        }
    }

    #[cfg(any(
        test,
        all(
            any(feature = "webview", feature = "webview-wayland-gtk4", any()),
            any(target_os = "linux", target_os = "freebsd")
        )
    ))]
    pub(crate) fn with_top_level_origin(
        kind: WebViewPermissionKind,
        origin: Option<SharedString>,
    ) -> Self {
        let origin_source = if origin.is_some() {
            WebViewPermissionOriginSource::TopLevelDocument
        } else {
            WebViewPermissionOriginSource::Unavailable
        };
        Self {
            kind,
            origin,
            origin_source,
            frame: WebViewPermissionFrame::Unknown,
            user_gesture: None,
        }
    }
}

/// App decision for both native WebView permission policy and JavaScript
/// permission preflighting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebViewPermissionDecision {
    /// Continue with the embedded browser's normal permission behavior. On
    /// Linux, the underlying WebKitGTK default is to deny the request.
    #[default]
    Default,
    /// Grant the native request, or allow a preflighted page API to continue.
    Allow,
    /// Deny the native request, or block a preflighted page API.
    Deny,
}

impl WebViewPermissionDecision {
    pub(crate) fn as_bridge_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
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
    #[cfg(all(any(), any(target_os = "linux", target_os = "freebsd")))]
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

pub(crate) type WebViewPermissionHandler = Arc<
    dyn Fn(WebViewNativePermissionRequest) -> WebViewPermissionDecision + Send + Sync + 'static,
>;

pub(crate) type WebViewCookieCallback =
    Rc<dyn Fn(Result<Vec<WebViewCookie>, SharedString>) + 'static>;

pub(crate) type WebViewCookieMutationCallback = Rc<dyn Fn(Result<(), SharedString>) + 'static>;

pub(crate) type WebViewUrlCallback = Rc<dyn Fn(Result<SharedString, SharedString>) + 'static>;

pub(crate) type WebViewDevToolsStateCallback = Rc<dyn Fn(Result<bool, SharedString>) + 'static>;

pub(crate) type WebViewJavaScriptResultCallback =
    Arc<dyn Fn(Result<SharedString, SharedString>) + Send + Sync + 'static>;

#[derive(Clone)]
#[cfg_attr(not(feature = "webview"), allow(dead_code))]
pub(crate) struct PlatformWebView {
    /// Stable identity of this rendered element instance. Unlike `id`, this is
    /// qualified by the complete element path and is safe to use as a native
    /// host-map key when separate subtrees reuse the same local element id.
    pub(crate) instance_id: SharedString,
    /// Public command id used by [`crate::WebViewController`]. This must be
    /// unique among the WebViews rendered into a single window.
    pub(crate) id: SharedString,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) url: SharedString,
    pub(crate) html: Option<SharedString>,
    pub(crate) visible: bool,
    pub(crate) opacity: f32,
    pub(crate) storage_key: Option<SharedString>,
    pub(crate) user_agent: Option<SharedString>,
    pub(crate) injected_css: Vec<SharedString>,
    pub(crate) injected_javascript: Vec<SharedString>,
    pub(crate) request_headers: Option<http_client::http::HeaderMap>,
    pub(crate) javascript_disabled: bool,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) general_autofill: Option<bool>,
    pub(crate) background_color: Option<Rgba>,
    // Browser iframe hosts cannot control the browser's developer tools or
    // native zoom shortcuts, but retain the declarative values so one view
    // description remains portable across native and browser backends.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) devtools: bool,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) zoom_hotkeys_enabled: bool,
    pub(crate) media_autoplay: Option<bool>,
    pub(crate) focused: Option<bool>,
    pub(crate) clipboard_access: bool,
    /// App-owned URL schemes visible while this WebView description was
    /// painted. Native hosts use this immutable snapshot to register protocol
    /// callbacks before navigating, without re-borrowing `App` from a platform
    /// callback.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) custom_protocol_schemes: Vec<SharedString>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) browser_policy: BrowserWebViewPolicy,
    pub(crate) async_window: AsyncWindowContext,
    pub(crate) message_handler: Option<WebViewMessageHandler>,
    pub(crate) navigation_handler: Option<WebViewNavigationHandler>,
    // These callbacks require host hooks that HTML iframes do not expose.
    // Their browser support level is reported explicitly by
    // `WebViewCapabilityReport`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) new_window_handler: Option<WebViewNewWindowHandler>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) download_started_handler: Option<WebViewDownloadStartedHandler>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) download_completed_handler: Option<WebViewDownloadCompletedHandler>,
    pub(crate) document_title_changed_handler: Option<WebViewDocumentTitleChangedHandler>,
    pub(crate) page_load_handler: Option<WebViewPageLoadHandler>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) drag_drop_handler: Option<WebViewDragDropHandler>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) permission_handler: Option<WebViewPermissionHandler>,
}

pub(crate) fn webview_instance_id(
    global_id: Option<&GlobalElementId>,
    fallback_id: &SharedString,
) -> SharedString {
    let Some(global_id) = global_id else {
        return fallback_id.clone();
    };

    // GlobalElementId's Display form is intended for diagnostics. Length-prefix
    // every component here so ids containing `.` cannot alias a different path.
    let mut encoded = String::new();
    for component in global_id.0.iter() {
        let component = format!("{component:?}");
        let _ = write!(encoded, "{}:{component};", component.len());
    }
    encoded.into()
}

#[derive(Clone)]
#[cfg_attr(not(feature = "webview"), allow(dead_code))]
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

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementId;
    use smallvec::smallvec;

    #[test]
    fn webview_instance_ids_are_qualified_by_the_element_path() {
        let first = GlobalElementId(smallvec![
            ElementId::from("left"),
            ElementId::from("browser")
        ]);
        let second = GlobalElementId(smallvec![
            ElementId::from("right"),
            ElementId::from("browser")
        ]);
        let fallback: SharedString = "browser".into();

        assert_ne!(
            webview_instance_id(Some(&first), &fallback),
            webview_instance_id(Some(&second), &fallback)
        );
    }

    #[test]
    fn webview_instance_id_encoding_cannot_alias_delimited_components() {
        let first = GlobalElementId(smallvec![ElementId::from("a.b"), ElementId::from("c")]);
        let second = GlobalElementId(smallvec![ElementId::from("a"), ElementId::from("b.c")]);
        let fallback: SharedString = "unused".into();

        assert_eq!(first.to_string(), second.to_string());
        assert_ne!(
            webview_instance_id(Some(&first), &fallback),
            webview_instance_id(Some(&second), &fallback)
        );
    }

    #[test]
    fn webview_instance_id_encoding_preserves_element_id_variants() {
        let integer = GlobalElementId(smallvec![ElementId::Integer(1)]);
        let name = GlobalElementId(smallvec![ElementId::from("1")]);
        let fallback: SharedString = "unused".into();

        assert_eq!(integer.to_string(), name.to_string());
        assert_ne!(
            webview_instance_id(Some(&integer), &fallback),
            webview_instance_id(Some(&name), &fallback)
        );
    }

    #[test]
    fn webview_instance_id_uses_fallback_without_a_global_path() {
        let fallback: SharedString = "browser".into();
        assert_eq!(webview_instance_id(None, &fallback), fallback);
    }

    #[test]
    fn webview_capability_matrix_is_complete_for_every_backend() {
        for backend in [
            WebViewBackend::BrowserIframe,
            WebViewBackend::MacOsWkWebView,
            WebViewBackend::WindowsWebView2,
            WebViewBackend::LinuxX11WebKitGtk,
            WebViewBackend::LinuxX11WebKitGtk6,
            WebViewBackend::LinuxWaylandWebKitGtk6,
            WebViewBackend::LinuxWaylandUnavailable,
            WebViewBackend::LinuxHeadlessUnavailable,
            WebViewBackend::Disabled,
            WebViewBackend::UnsupportedPlatform,
        ] {
            let report = WebViewCapabilityReport::for_backend(backend);
            assert_eq!(report.capabilities.len(), WEBVIEW_CAPABILITIES.len());
            for capability in WEBVIEW_CAPABILITIES {
                let entry = report.entry(capability).unwrap();
                assert_eq!(entry.capability, capability);
                assert!(!entry.note.is_empty());
            }
        }
    }

    #[test]
    fn webview_capability_matrix_exposes_portable_and_backend_owned_boundaries() {
        let browser = WebViewCapabilityReport::for_backend(WebViewBackend::BrowserIframe);
        assert_eq!(
            browser.support_for(WebViewCapability::Focus),
            SupportLevel::Full
        );
        assert_eq!(
            browser.support_for(WebViewCapability::RequestHeaders),
            SupportLevel::Unsupported
        );
        assert_eq!(
            browser.support_for(WebViewCapability::ProfileIsolation),
            SupportLevel::Unsupported
        );

        let windows = WebViewCapabilityReport::for_backend(WebViewBackend::WindowsWebView2);
        assert_eq!(
            windows.support_for(WebViewCapability::RequestHeaders),
            SupportLevel::Full
        );

        let macos = WebViewCapabilityReport::for_backend(WebViewBackend::MacOsWkWebView);
        assert_eq!(
            macos.support_for(WebViewCapability::CustomProtocols),
            SupportLevel::Full
        );
        assert_eq!(
            windows.support_for(WebViewCapability::DragDrop),
            SupportLevel::Partial
        );
        assert_eq!(
            windows.support_for(WebViewCapability::CustomProtocols),
            SupportLevel::Full
        );

        let x11 = WebViewCapabilityReport::for_backend(WebViewBackend::LinuxX11WebKitGtk);
        assert_eq!(
            x11.support_for(WebViewCapability::CustomProtocols),
            SupportLevel::Full
        );

        let gtk4_x11 = WebViewCapabilityReport::for_backend(WebViewBackend::LinuxX11WebKitGtk6);
        assert_eq!(
            gtk4_x11.support_for(WebViewCapability::CustomProtocols),
            SupportLevel::Full
        );
        assert_eq!(
            gtk4_x11.support_for(WebViewCapability::DragDrop),
            SupportLevel::Full
        );

        let gtk4_wayland =
            WebViewCapabilityReport::for_backend(WebViewBackend::LinuxWaylandWebKitGtk6);
        assert_eq!(
            gtk4_wayland.support_for(WebViewCapability::Composition),
            SupportLevel::Partial
        );
        assert_eq!(
            gtk4_wayland.support_for(WebViewCapability::CustomProtocols),
            SupportLevel::Full
        );
        assert_eq!(
            gtk4_wayland.support_for(WebViewCapability::DragDrop),
            SupportLevel::Full
        );

        let wayland = WebViewCapabilityReport::for_backend(WebViewBackend::LinuxWaylandUnavailable);
        assert!(!wayland.is_available(WebViewCapability::Composition));
        assert_eq!(wayland.unavailable().len(), WEBVIEW_CAPABILITIES.len());
    }

    #[test]
    fn browser_webview_downloads_are_explicitly_opt_in() {
        let default_policy = BrowserWebViewPolicy::default();
        assert!(!default_policy.downloads);
        assert!(BrowserWebViewPolicy::strict().downloads(true).downloads);
    }
}
