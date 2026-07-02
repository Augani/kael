use std::{
    any::{TypeId, type_name},
    cell::{BorrowMutError, Ref, RefCell, RefMut},
    fmt::Write as _,
    fs,
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    path::{Component, Path, PathBuf},
    rc::{Rc, Weak},
    sync::{Arc, atomic::Ordering::SeqCst},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use derive_more::{Deref, DerefMut};
use futures::{
    Future, FutureExt,
    channel::oneshot,
    future::{LocalBoxFuture, Shared},
};
use itertools::Itertools;
use parking_lot::RwLock;
use slotmap::SlotMap;

pub use async_context::*;
use collections::{FxHashMap, FxHashSet, HashMap, VecDeque};
pub use context::*;
pub use entity_map::*;
use http_client::{HttpClient, Url};
use smallvec::SmallVec;
#[cfg(any(test, feature = "test-support"))]
pub use test_context::*;
use util::{ResultExt, debug_panic};

#[cfg(any(feature = "inspector", debug_assertions))]
use crate::InspectorElementRegistry;
use crate::{
    Action, ActionBuildError, ActionRegistry, Any, AnyView, AnyWindowHandle, AppContext, Asset,
    AssetSource, AttentionType, BackgroundExecutor, BiometricStatus, Bounds, Capability,
    ClipboardItem, ClipboardItemBuilder, CommandRegistry, CrashReport, CrashReporter,
    CrashReporterBuilder, CursorStyle, DialogOptions, DispatchPhase, DisplayId, DockMenuBuilder,
    EventEmitter, FileWatcher, FocusHandle, FocusMap, FocusedWindowInfo, FocusedWindowQueryBuilder,
    ForegroundExecutor, Global, GlobalHotkeyBuilder, GlobalHotkeySet, KeyBinding, KeyContext,
    Keymap, Keystroke, LayoutId, MediaKeyEvent, Menu, MenuBarBuilder, MenuItem,
    MessageDialogBuilder, NativeContextMenuBuilder, NetworkStatus, NotificationAction,
    NotificationBuilder, OpenDialogBuilder, OsInfo, OwnedMenu, PathPromptOptions, PathScope,
    PermissionBroker, PermissionResult, PermissionStatus, Pixels, Platform, PlatformDisplay,
    PlatformKeyboardLayout, PlatformKeyboardMapper, Point, PowerMode, PowerSaveBlockerKind,
    ProcessClass, ProcessId, PromptBuilder, PromptButton, PromptHandle, PromptLevel, Redo, Render,
    RenderImage, RenderablePromptHandle, Reservation, SaveDialogBuilder, ScreenCaptureSource,
    SharedString, ShellTarget, ShellTargetsBuilder, Size, SubscriberSet, Subscription, SvgRenderer,
    SystemPowerEvent, Task, TextSystem, Theme, ThreatModel, TrashRequest, TrashRequestBuilder,
    TrayIconEvent, TrayMenuBuilder, TrayMenuItem, Undo, Window, WindowAppearance, WindowHandle,
    WindowId, WindowInvalidator, WindowPosition, current_platform, hash, init_app_menus,
    media_capture::{CaptureManager, default_capture_manager},
    theme::{
        normalize_theme_path, register_theme_file_subscriber, retain_file_watcher,
        theme_file_event_matches_target,
    },
};

mod async_context;
mod context;
mod entity_map;
#[cfg(any(test, feature = "test-support"))]
mod test_context;

/// The duration for which futures returned from [Context::on_app_quit] can run before the application fully quits.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);

/// A typed request from the operating system to open a URL, deep link, or file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    raw: String,
    kind: OpenRequestKind,
}

impl OpenRequest {
    /// Classify a platform-provided open string.
    pub fn parse(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let kind = classify_open_request(&raw);
        Self { raw, kind }
    }

    /// The original string supplied by the platform.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The classified request kind.
    pub fn kind(&self) -> &OpenRequestKind {
        &self.kind
    }

    /// Return the URL scheme when this request has one.
    pub fn scheme(&self) -> Option<&str> {
        self.kind.scheme()
    }

    /// Return the file path when this request is a file-open request.
    pub fn file_path(&self) -> Option<&Path> {
        match &self.kind {
            OpenRequestKind::File { path } => Some(path.as_path()),
            _ => None,
        }
    }

    /// Whether this request should normally be routed through app-owned deep-link handlers.
    pub fn is_deep_link(&self) -> bool {
        matches!(self.kind, OpenRequestKind::DeepLink { .. })
    }
}

/// The classified kind of an OS open request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRequestKind {
    /// A `file://` URL or platform file-open request.
    File {
        /// The decoded local file path.
        path: PathBuf,
    },
    /// An app-owned URL scheme such as `myapp://settings`.
    DeepLink {
        /// The app-owned URL scheme.
        scheme: String,
    },
    /// A regular external URL such as `https://example.com` or `mailto:...`.
    Url {
        /// The external URL scheme.
        scheme: String,
    },
    /// A raw string that could not be classified safely.
    Unknown,
}

impl OpenRequestKind {
    /// Return the associated URL scheme, if any.
    pub fn scheme(&self) -> Option<&str> {
        match self {
            OpenRequestKind::File { .. } => Some("file"),
            OpenRequestKind::DeepLink { scheme } | OpenRequestKind::Url { scheme } => {
                Some(scheme.as_str())
            }
            OpenRequestKind::Unknown => None,
        }
    }
}

/// A callback for URLs matching one deep-link scheme.
pub struct DeepLinkRoute {
    scheme: String,
    callback: Box<dyn FnMut(String, &mut App)>,
}

impl DeepLinkRoute {
    /// Create a deep-link route for one URL scheme.
    pub fn new(
        scheme: impl Into<String>,
        callback: impl FnMut(String, &mut App) + 'static,
    ) -> Self {
        Self {
            scheme: scheme.into(),
            callback: Box::new(callback),
        }
    }

    /// The URL scheme this route handles.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    fn into_parts(self) -> (String, Box<dyn FnMut(String, &mut App)>) {
        (self.scheme, self.callback)
    }
}

/// Builder for grouped deep-link routes.
pub struct DeepLinkRouterBuilder {
    routes: Vec<DeepLinkRoute>,
}

impl DeepLinkRouterBuilder {
    /// Create an empty deep-link router builder.
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add a scheme-specific deep-link route.
    pub fn route(
        mut self,
        scheme: impl Into<String>,
        callback: impl FnMut(String, &mut App) + 'static,
    ) -> Self {
        self.routes.push(DeepLinkRoute::new(scheme, callback));
        self
    }

    /// Return the configured routes.
    pub fn routes(&self) -> &[DeepLinkRoute] {
        &self.routes
    }

    /// Validate every configured route.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.routes.is_empty(),
            "at least one deep-link route must be configured"
        );

        let mut schemes = std::collections::HashSet::new();
        for route in &self.routes {
            validate_url_scheme(route.scheme())?;
            anyhow::ensure!(
                schemes.insert(route.scheme()),
                "deep-link route scheme must be unique: {}",
                route.scheme()
            );
        }

        Ok(())
    }

    /// Build the validated route list.
    pub fn build_checked(self) -> Result<Vec<DeepLinkRoute>> {
        self.validate()?;
        Ok(self.routes)
    }

    /// Build the route list.
    pub fn build(self) -> Vec<DeepLinkRoute> {
        self.routes
    }
}

impl Default for DeepLinkRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<DeepLinkRoute> for DeepLinkRouterBuilder {
    fn from(value: DeepLinkRoute) -> Self {
        Self {
            routes: vec![value],
        }
    }
}

impl From<DeepLinkRouterBuilder> for Vec<DeepLinkRoute> {
    fn from(value: DeepLinkRouterBuilder) -> Self {
        value.build()
    }
}

/// A typed request for an app-owned custom protocol URL such as
/// `app://assets/logo.svg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProtocolRequest {
    raw_url: String,
    scheme: String,
    host: Option<String>,
    path: String,
    query: Option<String>,
}

impl CustomProtocolRequest {
    /// Parse and validate a custom protocol request URL.
    pub fn parse(url: impl Into<String>) -> Result<Self> {
        let raw_url = url.into();
        anyhow::ensure!(
            raw_url == raw_url.trim(),
            "custom protocol URL cannot have leading or trailing whitespace"
        );
        let parsed = Url::parse(&raw_url).context("custom protocol URL is invalid")?;
        validate_custom_protocol_scheme(parsed.scheme())?;
        Ok(Self {
            raw_url,
            scheme: parsed.scheme().to_string(),
            host: parsed.host_str().map(str::to_string),
            path: parsed.path().to_string(),
            query: parsed.query().map(str::to_string),
        })
    }

    /// The original URL string.
    pub fn raw_url(&self) -> &str {
        &self.raw_url
    }

    /// The custom URL scheme.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Optional URL host.
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// URL path, always beginning with `/` for hierarchical custom URLs.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Optional query string without the leading `?`.
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }
}

/// A validated response returned by a custom protocol handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProtocolResponse {
    /// HTTP-style status code.
    pub status: u16,
    /// MIME type for the response body.
    pub mime_type: String,
    /// Additional response headers.
    pub headers: Vec<(String, String)>,
    /// Response bytes.
    pub body: Vec<u8>,
}

impl CustomProtocolResponse {
    /// Build a UTF-8 HTML response.
    pub fn html(body: impl Into<String>) -> Result<Self> {
        CustomProtocolResponseBuilder::new("text/html; charset=utf-8")
            .body(body.into().into_bytes())
            .build_checked()
    }

    /// Build a UTF-8 plain text response.
    pub fn text(body: impl Into<String>) -> Result<Self> {
        CustomProtocolResponseBuilder::new("text/plain; charset=utf-8")
            .body(body.into().into_bytes())
            .build_checked()
    }

    /// Build a JSON response from bytes or a serialized JSON string.
    pub fn json(body: impl Into<Vec<u8>>) -> Result<Self> {
        CustomProtocolResponseBuilder::new("application/json")
            .body(body)
            .build_checked()
    }

    /// Build a binary response with a caller-provided MIME type.
    pub fn bytes(mime_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Result<Self> {
        CustomProtocolResponseBuilder::new(mime_type)
            .body(body)
            .build_checked()
    }

    /// Build a 404 response.
    pub fn not_found() -> Self {
        Self {
            status: 404,
            mime_type: "text/plain; charset=utf-8".to_string(),
            headers: Vec::new(),
            body: b"Not Found".to_vec(),
        }
    }

    /// Validate response status, MIME type, headers, and body shape.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            (100..=599).contains(&self.status),
            "custom protocol response status must be in 100..=599"
        );
        validate_mime_type(&self.mime_type, "custom protocol response MIME type")?;
        for (name, value) in &self.headers {
            validate_header_name(name)?;
            validate_header_value(value)?;
        }
        Ok(())
    }
}

/// Builder for custom protocol responses.
#[derive(Debug, Clone)]
pub struct CustomProtocolResponseBuilder {
    status: u16,
    mime_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl CustomProtocolResponseBuilder {
    /// Create a response builder for a MIME type.
    pub fn new(mime_type: impl Into<String>) -> Self {
        Self {
            status: 200,
            mime_type: mime_type.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Set the HTTP-style status code.
    pub fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Add a response header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Set response body bytes.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Validate and build the response.
    pub fn build_checked(self) -> Result<CustomProtocolResponse> {
        let response = CustomProtocolResponse {
            status: self.status,
            mime_type: self.mime_type,
            headers: self.headers,
            body: self.body,
        };
        response.validate()?;
        Ok(response)
    }
}

/// A checked resolver that maps app-owned custom protocol URLs to files under a root directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProtocolFileResolver {
    root: PathBuf,
    host: Option<String>,
    index_file: Option<String>,
    cache_control: Option<String>,
}

impl CustomProtocolFileResolver {
    /// Start building a file resolver rooted at a packaged or generated asset directory.
    pub fn builder(root: impl AsRef<Path>) -> CustomProtocolFileResolverBuilder {
        CustomProtocolFileResolverBuilder::new(root)
    }

    /// Return the root directory all resolved files must stay inside.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the optional host this resolver accepts.
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// Resolve a custom protocol request to a filesystem path under the root.
    pub fn resolve_path(&self, request: &CustomProtocolRequest) -> Result<Option<PathBuf>> {
        if let Some(expected_host) = &self.host
            && request.host() != Some(expected_host.as_str())
        {
            return Ok(None);
        }
        anyhow::ensure!(
            !custom_protocol_raw_path_has_parent_component(request.raw_url()),
            "custom protocol file path cannot escape resolver root: {}",
            request.path()
        );

        let request_path = percent_decode_open_component(request.path());
        let mut relative = PathBuf::new();
        for component in Path::new(&request_path).components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(segment) => relative.push(segment),
                Component::ParentDir | Component::Prefix(_) => {
                    anyhow::bail!(
                        "custom protocol file path cannot escape resolver root: {}",
                        request.path()
                    );
                }
            }
        }

        if relative.as_os_str().is_empty()
            && let Some(index_file) = &self.index_file
        {
            relative.push(index_file);
        }

        let candidate = self.root.join(relative);
        ensure_path_stays_under_root(&self.root, &candidate)?;
        Ok(candidate.is_file().then_some(candidate))
    }

    /// Resolve and read a custom protocol request into a checked response.
    pub fn response(&self, request: &CustomProtocolRequest) -> Result<CustomProtocolResponse> {
        let Some(path) = self.resolve_path(request)? else {
            return Ok(CustomProtocolResponse::not_found());
        };
        let body = fs::read(&path)
            .with_context(|| format!("failed to read custom protocol file {}", path.display()))?;
        let mut builder = CustomProtocolResponseBuilder::new(mime_type_for_path(&path)).body(body);
        if let Some(cache_control) = &self.cache_control {
            builder = builder.header("Cache-Control", cache_control);
        }
        builder.build_checked()
    }

    /// Build a checked route that serves files from this resolver.
    pub fn route(self, scheme: impl Into<String>) -> Result<CustomProtocolRoute> {
        let scheme = scheme.into();
        validate_custom_protocol_scheme(&scheme)?;
        let resolver = self;
        Ok(CustomProtocolRoute::new(scheme, move |request, _| {
            resolver.response(&request)
        }))
    }
}

/// Builder for safe custom-protocol file serving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProtocolFileResolverBuilder {
    root: PathBuf,
    host: Option<String>,
    index_file: Option<String>,
    cache_control: Option<String>,
    require_existing_root: bool,
    canonicalize_root: bool,
}

impl CustomProtocolFileResolverBuilder {
    /// Create a file resolver builder rooted at the supplied directory.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            host: None,
            index_file: None,
            cache_control: None,
            require_existing_root: false,
            canonicalize_root: false,
        }
    }

    /// Accept only requests for a specific URL host, such as `assets` in `app://assets/logo.svg`.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Serve this file when the request path is `/`.
    pub fn index_file(mut self, index_file: impl Into<String>) -> Self {
        self.index_file = Some(index_file.into());
        self
    }

    /// Add a Cache-Control header to successful file responses.
    pub fn cache_control(mut self, value: impl Into<String>) -> Self {
        self.cache_control = Some(value.into());
        self
    }

    /// Require the root path to exist and be a directory.
    pub fn require_existing_root(mut self) -> Self {
        self.require_existing_root = true;
        self
    }

    /// Canonicalize the root directory while building.
    pub fn canonicalize_root(mut self) -> Self {
        self.canonicalize_root = true;
        self.require_existing_root = true;
        self
    }

    /// Validate the resolver configuration.
    pub fn validate(&self) -> Result<()> {
        validate_custom_protocol_root(&self.root, self.require_existing_root)?;
        if let Some(host) = &self.host {
            validate_custom_protocol_host(host)?;
        }
        if let Some(index_file) = &self.index_file {
            validate_custom_protocol_relative_file(index_file, "custom protocol index file")?;
        }
        if let Some(cache_control) = &self.cache_control {
            validate_header_value(cache_control)?;
        }
        Ok(())
    }

    /// Build a checked file resolver.
    pub fn build_checked(mut self) -> Result<CustomProtocolFileResolver> {
        self.validate()?;
        if self.canonicalize_root {
            self.root = self.root.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize custom protocol root {}",
                    self.root.display()
                )
            })?;
        }
        Ok(CustomProtocolFileResolver {
            root: self.root,
            host: self.host,
            index_file: self.index_file,
            cache_control: self.cache_control,
        })
    }

    /// Build a checked custom protocol route for this file resolver.
    pub fn route_checked(self, scheme: impl Into<String>) -> Result<CustomProtocolRoute> {
        let scheme = scheme.into();
        validate_custom_protocol_scheme(&scheme)?;
        let resolver = self.build_checked()?;
        Ok(CustomProtocolRoute::new(scheme, move |request, _| {
            resolver.response(&request)
        }))
    }
}

type CustomProtocolHandler =
    Box<dyn FnMut(CustomProtocolRequest, &mut App) -> Result<CustomProtocolResponse>>;

/// A registered app-owned custom protocol route.
pub struct CustomProtocolRoute {
    scheme: String,
    handler: CustomProtocolHandler,
}

impl CustomProtocolRoute {
    /// Create a custom protocol route.
    pub fn new(
        scheme: impl Into<String>,
        handler: impl FnMut(CustomProtocolRequest, &mut App) -> Result<CustomProtocolResponse> + 'static,
    ) -> Self {
        Self {
            scheme: scheme.into(),
            handler: Box::new(handler),
        }
    }

    /// The custom protocol scheme.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    fn into_parts(self) -> (String, CustomProtocolHandler) {
        (self.scheme, self.handler)
    }
}

/// Builder for grouped custom protocol routes.
pub struct CustomProtocolRouterBuilder {
    routes: Vec<CustomProtocolRoute>,
}

impl CustomProtocolRouterBuilder {
    /// Create an empty custom protocol router builder.
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add one app-owned custom protocol route.
    pub fn route(
        mut self,
        scheme: impl Into<String>,
        handler: impl FnMut(CustomProtocolRequest, &mut App) -> Result<CustomProtocolResponse> + 'static,
    ) -> Self {
        self.routes.push(CustomProtocolRoute::new(scheme, handler));
        self
    }

    /// Return the configured routes.
    pub fn routes(&self) -> &[CustomProtocolRoute] {
        &self.routes
    }

    /// Validate every configured route.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.routes.is_empty(),
            "at least one custom protocol route must be configured"
        );

        let mut schemes = std::collections::HashSet::new();
        for route in &self.routes {
            validate_custom_protocol_scheme(route.scheme())?;
            anyhow::ensure!(
                schemes.insert(route.scheme()),
                "custom protocol route scheme must be unique: {}",
                route.scheme()
            );
        }

        Ok(())
    }

    /// Build the validated route list.
    pub fn build_checked(self) -> Result<Vec<CustomProtocolRoute>> {
        self.validate()?;
        Ok(self.routes)
    }

    /// Build the route list without validation.
    pub fn build(self) -> Vec<CustomProtocolRoute> {
        self.routes
    }
}

impl Default for CustomProtocolRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<CustomProtocolRoute> for CustomProtocolRouterBuilder {
    fn from(value: CustomProtocolRoute) -> Self {
        Self {
            routes: vec![value],
        }
    }
}

impl From<CustomProtocolRouterBuilder> for Vec<CustomProtocolRoute> {
    fn from(value: CustomProtocolRouterBuilder) -> Self {
        value.build()
    }
}

/// Builder for writing one credential entry to the platform keychain.
#[derive(Debug, Clone, Default)]
pub struct CredentialBuilder {
    service: String,
    username: Option<String>,
    secret: Option<Vec<u8>>,
}

impl CredentialBuilder {
    /// Create a credential builder for a service or URL key.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            username: None,
            secret: None,
        }
    }

    /// Set the credential account name.
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set the credential secret bytes.
    pub fn secret(mut self, secret: impl Into<Vec<u8>>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Set the credential secret from UTF-8 text.
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.secret = Some(password.into().into_bytes());
        self
    }

    /// Return the configured service key.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Return the configured username, if any.
    pub fn configured_username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    /// Return the configured secret, if any.
    pub fn configured_secret(&self) -> Option<&[u8]> {
        self.secret.as_deref()
    }

    /// Validate that the service, username, and secret are present.
    pub fn validate(&self) -> Result<()> {
        validate_credential_service(&self.service)?;
        anyhow::ensure!(
            self.username
                .as_ref()
                .is_some_and(|username| !username.trim().is_empty()),
            "credential username cannot be empty"
        );
        if let Some(username) = &self.username {
            validate_credential_label(username, "credential username")?;
        }
        anyhow::ensure!(
            self.secret
                .as_ref()
                .is_some_and(|secret| !secret.is_empty()),
            "credential secret cannot be empty"
        );
        Ok(())
    }

    /// Build a validated credential request.
    pub fn build(self) -> Result<CredentialWriteRequest> {
        self.validate()?;
        Ok(CredentialWriteRequest {
            service: self.service,
            username: self.username.expect("validated username"),
            secret: self.secret.expect("validated secret"),
        })
    }
}

/// Builder for a validated credential service key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialServiceBuilder {
    service: String,
}

impl CredentialServiceBuilder {
    /// Create a credential service-key builder.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Return the configured service key.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Validate the service key before using the platform keychain.
    pub fn validate(&self) -> Result<()> {
        validate_credential_service(&self.service)
    }

    /// Build the validated service key.
    pub fn build(self) -> Result<String> {
        self.validate()?;
        Ok(self.service)
    }
}

/// A validated credential entry stored in the platform keychain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialWriteRequest {
    service: String,
    username: String,
    secret: Vec<u8>,
}

impl CredentialWriteRequest {
    /// Service or URL key for this credential.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Account name for this credential.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Secret bytes for this credential.
    pub fn secret(&self) -> &[u8] {
        &self.secret
    }

    fn into_parts(self) -> (String, String, Vec<u8>) {
        (self.service, self.username, self.secret)
    }
}

/// A credential read from the platform keychain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredential {
    username: String,
    secret: Vec<u8>,
}

impl StoredCredential {
    /// Create a stored credential result.
    pub fn new(username: impl Into<String>, secret: impl Into<Vec<u8>>) -> Self {
        Self {
            username: username.into(),
            secret: secret.into(),
        }
    }

    /// Account name for this credential.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Secret bytes for this credential.
    pub fn secret(&self) -> &[u8] {
        &self.secret
    }
}

fn validate_credential_service(service: &str) -> Result<()> {
    validate_credential_label(service, "credential service")?;
    anyhow::ensure!(
        !service.contains('\0'),
        "credential service cannot contain NUL characters"
    );
    Ok(())
}

fn validate_credential_label(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.len() <= 512,
        "{label} cannot be longer than 512 bytes"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

/// Builder for launch-at-login settings.
#[derive(Debug, Clone)]
pub struct AutoLaunchBuilder {
    app_id: String,
    enabled: bool,
}

impl AutoLaunchBuilder {
    /// Enable launch-at-login for the application identifier.
    pub fn enable(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            enabled: true,
        }
    }

    /// Disable launch-at-login for the application identifier.
    pub fn disable(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            enabled: false,
        }
    }

    /// Set whether launch-at-login should be enabled.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Return the configured application identifier.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Return the requested launch-at-login state.
    pub fn requested_enabled(&self) -> bool {
        self.enabled
    }

    /// Validate the application identifier.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.app_id.trim().is_empty(),
            "auto-launch app id cannot be empty"
        );
        anyhow::ensure!(
            self.app_id == self.app_id.trim(),
            "auto-launch app id cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            !self
                .app_id
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace()),
            "auto-launch app id cannot contain whitespace or control characters"
        );
        Ok(())
    }

    /// Build the validated app id and requested state.
    pub fn build_checked(self) -> Result<(String, bool)> {
        self.validate()?;
        Ok((self.app_id, self.enabled))
    }
}

/// Result returned after configuring launch-at-login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoLaunchStatus {
    app_id: String,
    enabled: bool,
}

impl AutoLaunchStatus {
    /// Application identifier used for the platform setting.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Whether the platform reports launch-at-login as enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Well-known application path roles, similar to Electron's `app.getPath(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppPathRole {
    /// User data owned by the app, such as databases and durable state.
    Data,
    /// User-editable configuration owned by the app.
    Config,
    /// Rebuildable cache data owned by the app.
    Cache,
    /// App log output.
    Logs,
    /// Temporary files scoped to this app.
    Temp,
    /// The user's downloads directory.
    Downloads,
}

impl AppPathRole {
    /// Stable role key for diagnostics and generated manifests.
    pub fn key(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Config => "config",
            Self::Cache => "cache",
            Self::Logs => "logs",
            Self::Temp => "temp",
            Self::Downloads => "downloads",
        }
    }
}

/// Resolved application paths for a validated app identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPathSet {
    app_id: String,
    paths: Vec<(AppPathRole, PathBuf)>,
}

impl AppPathSet {
    /// The application identifier used to scope app-owned paths.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// All resolved role/path pairs, in builder order.
    pub fn paths(&self) -> &[(AppPathRole, PathBuf)] {
        &self.paths
    }

    /// Return the path for a role when it was requested.
    pub fn get(&self, role: AppPathRole) -> Option<&Path> {
        self.paths
            .iter()
            .find_map(|(candidate, path)| (*candidate == role).then_some(path.as_path()))
    }

    /// User data directory.
    pub fn data_dir(&self) -> Option<&Path> {
        self.get(AppPathRole::Data)
    }

    /// User configuration directory.
    pub fn config_dir(&self) -> Option<&Path> {
        self.get(AppPathRole::Config)
    }

    /// Rebuildable cache directory.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.get(AppPathRole::Cache)
    }

    /// Log directory.
    pub fn logs_dir(&self) -> Option<&Path> {
        self.get(AppPathRole::Logs)
    }

    /// App-scoped temporary directory.
    pub fn temp_dir(&self) -> Option<&Path> {
        self.get(AppPathRole::Temp)
    }

    /// User downloads directory.
    pub fn downloads_dir(&self) -> Option<&Path> {
        self.get(AppPathRole::Downloads)
    }
}

/// Builder for resolving common app-owned filesystem locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPathBuilder {
    app_id: String,
    roles: Vec<AppPathRole>,
    create_dirs: bool,
}

impl AppPathBuilder {
    /// Create an app path builder for a stable app identifier.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            roles: Vec::new(),
            create_dirs: false,
        }
    }

    /// Add one well-known path role.
    pub fn role(mut self, role: AppPathRole) -> Self {
        self.roles.push(role);
        self
    }

    /// Add multiple well-known path roles.
    pub fn roles(mut self, roles: impl IntoIterator<Item = AppPathRole>) -> Self {
        self.roles.extend(roles);
        self
    }

    /// Request the common app-owned storage roles.
    pub fn app_storage(self) -> Self {
        self.roles([
            AppPathRole::Data,
            AppPathRole::Config,
            AppPathRole::Cache,
            AppPathRole::Logs,
            AppPathRole::Temp,
        ])
    }

    /// Request app-owned storage plus the user's downloads directory.
    pub fn all_common(self) -> Self {
        self.app_storage().role(AppPathRole::Downloads)
    }

    /// Create resolved directories when they do not already exist.
    pub fn create_dirs(mut self) -> Self {
        self.create_dirs = true;
        self
    }

    /// Create resolved directories when `enabled` is true.
    pub fn create_dirs_if(mut self, enabled: bool) -> Self {
        self.create_dirs = enabled;
        self
    }

    /// Return the configured app identifier.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Return the configured path roles.
    pub fn configured_roles(&self) -> &[AppPathRole] {
        &self.roles
    }

    /// Whether missing resolved directories will be created.
    pub fn creates_dirs(&self) -> bool {
        self.create_dirs
    }

    /// Validate the app identifier and requested roles.
    pub fn validate(&self) -> Result<()> {
        validate_app_path_id(&self.app_id)?;
        anyhow::ensure!(
            !self.roles.is_empty(),
            "app path builder must request at least one role"
        );

        for (index, role) in self.roles.iter().enumerate() {
            anyhow::ensure!(
                !self.roles[..index].contains(role),
                "app path role requested more than once: {}",
                role.key()
            );
        }

        Ok(())
    }

    /// Resolve and optionally create the requested paths.
    pub fn build_checked(self) -> Result<AppPathSet> {
        self.validate()?;

        let mut paths = Vec::with_capacity(self.roles.len());
        for role in self.roles {
            let path = resolve_app_path_role(&self.app_id, role)?;
            if self.create_dirs {
                fs::create_dir_all(&path).with_context(|| {
                    format!(
                        "failed to create app path {} at {}",
                        role.key(),
                        path.display()
                    )
                })?;
            }
            paths.push((role, path));
        }

        Ok(AppPathSet {
            app_id: self.app_id,
            paths,
        })
    }
}

/// Storage class for app-owned persistent or rebuildable data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppStorageKind {
    /// JSON settings/preferences file.
    SettingsJson,
    /// SQLite or SQLite-compatible database file.
    SqliteDatabase,
    /// App-owned key-value store directory or file.
    KeyValueStore,
    /// Rebuildable blob/object cache.
    BlobCache,
    /// App log file.
    LogFile,
    /// Temporary workspace directory.
    TempWorkspace,
    /// App-defined storage kind.
    Custom(String),
}

impl AppStorageKind {
    /// Stable kind key for diagnostics and generated docs.
    pub fn key(&self) -> &str {
        match self {
            Self::SettingsJson => "settings-json",
            Self::SqliteDatabase => "sqlite-database",
            Self::KeyValueStore => "key-value-store",
            Self::BlobCache => "blob-cache",
            Self::LogFile => "log-file",
            Self::TempWorkspace => "temp-workspace",
            Self::Custom(kind) => kind,
        }
    }

    fn validate(&self) -> Result<()> {
        if let Self::Custom(kind) = self {
            validate_app_storage_id(kind, "app storage kind")?;
        }
        Ok(())
    }
}

/// Durability policy for an app storage entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStorageDurability {
    /// User data that should survive app upgrades and normal cleanup.
    Durable,
    /// Data that can be regenerated and may be cleared under pressure.
    Rebuildable,
    /// Short-lived data scoped to the current run or task.
    Temporary,
}

/// One checked app-owned storage location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorageEntry {
    id: String,
    kind: AppStorageKind,
    durability: AppStorageDurability,
    role: AppPathRole,
    relative_path: PathBuf,
    absolute_path: PathBuf,
    max_bytes: Option<u64>,
    sensitive: bool,
}

impl AppStorageEntry {
    /// Stable app-defined storage id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Storage class.
    pub fn kind(&self) -> &AppStorageKind {
        &self.kind
    }

    /// Durability policy.
    pub fn durability(&self) -> AppStorageDurability {
        self.durability
    }

    /// App path role used as the base directory.
    pub fn role(&self) -> AppPathRole {
        self.role
    }

    /// Relative path under the role directory.
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Absolute resolved storage path.
    pub fn absolute_path(&self) -> &Path {
        &self.absolute_path
    }

    /// Optional storage quota/expectation.
    pub fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    /// Whether diagnostics should redact this entry by default.
    pub fn is_sensitive(&self) -> bool {
        self.sensitive
    }

    /// Capability required by code that writes to this storage entry.
    pub fn write_capability(&self) -> Capability {
        Capability::FilesystemWrite {
            scope: PathScope::AppData,
        }
    }

    /// Capability required by code that reads from this storage entry.
    pub fn read_capability(&self) -> Capability {
        Capability::FilesystemRead {
            scope: PathScope::AppData,
        }
    }
}

/// Checked plan for app-owned settings, databases, caches, logs, and temp data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStoragePlan {
    app_id: String,
    paths: AppPathSet,
    entries: Vec<AppStorageEntry>,
}

impl AppStoragePlan {
    /// Application identifier used to scope storage.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Resolved app path set backing the plan.
    pub fn paths(&self) -> &AppPathSet {
        &self.paths
    }

    /// Storage entries in builder order.
    pub fn entries(&self) -> &[AppStorageEntry] {
        &self.entries
    }

    /// Look up an entry by id.
    pub fn entry(&self, id: &str) -> Option<&AppStorageEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Return entries with a specific durability policy.
    pub fn entries_with_durability(
        &self,
        durability: AppStorageDurability,
    ) -> Vec<&AppStorageEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.durability == durability)
            .collect()
    }

    /// Total declared quota across entries that specify one.
    pub fn declared_max_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(AppStorageEntry::max_bytes)
            .sum()
    }
}

/// Builder for one app storage entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorageEntryBuilder {
    id: String,
    kind: AppStorageKind,
    durability: AppStorageDurability,
    role: AppPathRole,
    relative_path: PathBuf,
    max_bytes: Option<u64>,
    sensitive: bool,
}

impl AppStorageEntryBuilder {
    /// Create an entry builder.
    pub fn new(
        id: impl Into<String>,
        kind: AppStorageKind,
        role: AppPathRole,
        relative_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            durability: AppStorageDurability::Durable,
            role,
            relative_path: relative_path.into(),
            max_bytes: None,
            sensitive: false,
        }
    }

    /// JSON settings/preferences file under the config directory.
    pub fn settings_json(id: impl Into<String>, file_name: impl Into<PathBuf>) -> Self {
        Self::new(
            id,
            AppStorageKind::SettingsJson,
            AppPathRole::Config,
            file_name,
        )
        .max_bytes(10 * 1024 * 1024)
    }

    /// SQLite database file under the data directory.
    pub fn sqlite_database(id: impl Into<String>, file_name: impl Into<PathBuf>) -> Self {
        Self::new(
            id,
            AppStorageKind::SqliteDatabase,
            AppPathRole::Data,
            file_name,
        )
        .max_bytes(2 * 1024 * 1024 * 1024)
    }

    /// Key-value store under the data directory.
    pub fn key_value_store(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::new(id, AppStorageKind::KeyValueStore, AppPathRole::Data, path)
            .max_bytes(512 * 1024 * 1024)
    }

    /// Rebuildable blob cache under the cache directory.
    pub fn blob_cache(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::new(id, AppStorageKind::BlobCache, AppPathRole::Cache, path)
            .rebuildable()
            .max_bytes(4 * 1024 * 1024 * 1024)
    }

    /// App log file under the logs directory.
    pub fn log_file(id: impl Into<String>, file_name: impl Into<PathBuf>) -> Self {
        Self::new(id, AppStorageKind::LogFile, AppPathRole::Logs, file_name)
            .rebuildable()
            .max_bytes(256 * 1024 * 1024)
    }

    /// Temporary workspace under the app temp directory.
    pub fn temp_workspace(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::new(id, AppStorageKind::TempWorkspace, AppPathRole::Temp, path)
            .temporary()
            .max_bytes(1024 * 1024 * 1024)
    }

    /// Mark the entry durable.
    pub fn durable(mut self) -> Self {
        self.durability = AppStorageDurability::Durable;
        self
    }

    /// Mark the entry rebuildable.
    pub fn rebuildable(mut self) -> Self {
        self.durability = AppStorageDurability::Rebuildable;
        self
    }

    /// Mark the entry temporary.
    pub fn temporary(mut self) -> Self {
        self.durability = AppStorageDurability::Temporary;
        self
    }

    /// Set the optional storage quota/expectation.
    pub fn max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// Clear the storage quota/expectation.
    pub fn unlimited_bytes(mut self) -> Self {
        self.max_bytes = None;
        self
    }

    /// Mark the entry as sensitive for diagnostics.
    pub fn sensitive(mut self) -> Self {
        self.sensitive = true;
        self
    }

    fn validate(&self) -> Result<()> {
        validate_app_storage_id(&self.id, "app storage id")?;
        self.kind.validate()?;
        anyhow::ensure!(
            self.role != AppPathRole::Downloads,
            "app storage entries cannot use the downloads directory"
        );
        validate_app_storage_relative_path(&self.relative_path)?;
        if let Some(max_bytes) = self.max_bytes {
            anyhow::ensure!(
                max_bytes > 0,
                "app storage max bytes must be greater than zero"
            );
            anyhow::ensure!(
                max_bytes <= 1_099_511_627_776,
                "app storage max bytes cannot exceed 1 TiB"
            );
        }
        Ok(())
    }

    fn build_checked(&self, paths: &AppPathSet) -> Result<AppStorageEntry> {
        self.validate()?;
        let base = paths
            .get(self.role)
            .with_context(|| format!("app storage role not resolved: {}", self.role.key()))?;
        Ok(AppStorageEntry {
            id: self.id.clone(),
            kind: self.kind.clone(),
            durability: self.durability,
            role: self.role,
            relative_path: self.relative_path.clone(),
            absolute_path: base.join(&self.relative_path),
            max_bytes: self.max_bytes,
            sensitive: self.sensitive,
        })
    }
}

/// Builder for checked app-owned storage plans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStoragePlanBuilder {
    app_id: String,
    entries: Vec<AppStorageEntryBuilder>,
    create_dirs: bool,
}

impl AppStoragePlanBuilder {
    /// Create an empty storage plan builder.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            entries: Vec::new(),
            create_dirs: false,
        }
    }

    /// Add a storage entry.
    pub fn entry(mut self, entry: AppStorageEntryBuilder) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add a settings JSON file.
    pub fn settings_json(mut self, id: impl Into<String>, file_name: impl Into<PathBuf>) -> Self {
        self.entries
            .push(AppStorageEntryBuilder::settings_json(id, file_name));
        self
    }

    /// Add a SQLite database file.
    pub fn sqlite_database(mut self, id: impl Into<String>, file_name: impl Into<PathBuf>) -> Self {
        self.entries
            .push(AppStorageEntryBuilder::sqlite_database(id, file_name));
        self
    }

    /// Add a key-value store path.
    pub fn key_value_store(mut self, id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.entries
            .push(AppStorageEntryBuilder::key_value_store(id, path));
        self
    }

    /// Add a rebuildable blob cache path.
    pub fn blob_cache(mut self, id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.entries
            .push(AppStorageEntryBuilder::blob_cache(id, path));
        self
    }

    /// Add an app log file.
    pub fn log_file(mut self, id: impl Into<String>, file_name: impl Into<PathBuf>) -> Self {
        self.entries
            .push(AppStorageEntryBuilder::log_file(id, file_name));
        self
    }

    /// Add a temporary workspace path.
    pub fn temp_workspace(mut self, id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.entries
            .push(AppStorageEntryBuilder::temp_workspace(id, path));
        self
    }

    /// Create resolved role directories when building.
    pub fn create_dirs(mut self) -> Self {
        self.create_dirs = true;
        self
    }

    /// Return configured entries.
    pub fn configured_entries(&self) -> &[AppStorageEntryBuilder] {
        &self.entries
    }

    /// Validate the storage plan.
    pub fn validate(&self) -> Result<()> {
        validate_app_path_id(&self.app_id)?;
        anyhow::ensure!(
            !self.entries.is_empty(),
            "app storage plan must include at least one entry"
        );
        let mut seen = std::collections::HashSet::new();
        for entry in &self.entries {
            entry.validate()?;
            anyhow::ensure!(
                seen.insert(entry.id.clone()),
                "app storage id configured more than once: {}",
                entry.id
            );
        }
        Ok(())
    }

    /// Build a checked storage plan and resolve role paths.
    pub fn build_checked(self) -> Result<AppStoragePlan> {
        self.validate()?;
        let roles = self
            .entries
            .iter()
            .map(|entry| entry.role)
            .unique()
            .collect::<Vec<_>>();
        let paths = AppPathBuilder::new(&self.app_id)
            .roles(roles)
            .create_dirs_if(self.create_dirs)
            .build_checked()?;
        let entries = self
            .entries
            .iter()
            .map(|entry| entry.build_checked(&paths))
            .collect::<Result<Vec<_>>>()?;
        Ok(AppStoragePlan {
            app_id: self.app_id,
            paths,
            entries,
        })
    }
}

/// Validated application identity metadata for About, diagnostics, and support UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMetadata {
    name: String,
    version: Option<String>,
    build: Option<String>,
    identifier: Option<String>,
    website_url: Option<String>,
    support_url: Option<String>,
    copyright: Option<String>,
    license: Option<String>,
    credits: Option<String>,
}

impl AppMetadata {
    /// User-facing application name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// User-facing semantic version string.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Build number, commit, or channel label.
    pub fn build(&self) -> Option<&str> {
        self.build.as_deref()
    }

    /// Stable bundle/package identifier.
    pub fn identifier(&self) -> Option<&str> {
        self.identifier.as_deref()
    }

    /// Product or marketing website.
    pub fn website_url(&self) -> Option<&str> {
        self.website_url.as_deref()
    }

    /// Support, help, or issue-reporting URL.
    pub fn support_url(&self) -> Option<&str> {
        self.support_url.as_deref()
    }

    /// Copyright text.
    pub fn copyright(&self) -> Option<&str> {
        self.copyright.as_deref()
    }

    /// License text.
    pub fn license(&self) -> Option<&str> {
        self.license.as_deref()
    }

    /// Credits or acknowledgements text.
    pub fn credits(&self) -> Option<&str> {
        self.credits.as_deref()
    }

    /// Compact display label such as `Kael Studio 1.2.3`.
    pub fn display_title(&self) -> String {
        match &self.version {
            Some(version) => format!("{} {}", self.name, version),
            None => self.name.clone(),
        }
    }

    /// Build an informational native dialog for this metadata.
    pub fn about_dialog(&self) -> MessageDialogBuilder {
        let mut detail = Vec::new();
        if let Some(build) = &self.build {
            detail.push(format!("Build: {build}"));
        }
        if let Some(identifier) = &self.identifier {
            detail.push(format!("Identifier: {identifier}"));
        }
        if let Some(website_url) = &self.website_url {
            detail.push(format!("Website: {website_url}"));
        }
        if let Some(support_url) = &self.support_url {
            detail.push(format!("Support: {support_url}"));
        }
        if let Some(copyright) = &self.copyright {
            detail.push(copyright.clone());
        }
        if let Some(license) = &self.license {
            detail.push(format!("License: {license}"));
        }
        if let Some(credits) = &self.credits {
            detail.push(credits.clone());
        }

        let dialog =
            MessageDialogBuilder::info(format!("About {}", self.name), self.display_title());
        if detail.is_empty() {
            dialog
        } else {
            dialog.detail(detail.join("\n"))
        }
    }
}

/// Builder for app identity metadata used by About panels and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMetadataBuilder {
    name: String,
    version: Option<String>,
    build: Option<String>,
    identifier: Option<String>,
    website_url: Option<String>,
    support_url: Option<String>,
    copyright: Option<String>,
    license: Option<String>,
    credits: Option<String>,
}

impl AppMetadataBuilder {
    /// Create metadata with a user-facing app name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            build: None,
            identifier: None,
            website_url: None,
            support_url: None,
            copyright: None,
            license: None,
            credits: None,
        }
    }

    /// Set the user-facing version string.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the build number, commit, or channel label.
    pub fn build(mut self, build: impl Into<String>) -> Self {
        self.build = Some(build.into());
        self
    }

    /// Set a stable bundle/package identifier.
    pub fn identifier(mut self, identifier: impl Into<String>) -> Self {
        self.identifier = Some(identifier.into());
        self
    }

    /// Set the product or marketing website.
    pub fn website_url(mut self, url: impl Into<String>) -> Self {
        self.website_url = Some(url.into());
        self
    }

    /// Set the support, help, or issue-reporting URL.
    pub fn support_url(mut self, url: impl Into<String>) -> Self {
        self.support_url = Some(url.into());
        self
    }

    /// Set copyright text.
    pub fn copyright(mut self, copyright: impl Into<String>) -> Self {
        self.copyright = Some(copyright.into());
        self
    }

    /// Set license text.
    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Set credits or acknowledgements text.
    pub fn credits(mut self, credits: impl Into<String>) -> Self {
        self.credits = Some(credits.into());
        self
    }

    /// Return the configured app name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Validate metadata before it reaches native chrome or support UI.
    pub fn validate(&self) -> Result<()> {
        validate_app_metadata_text(&self.name, "app name", 128, false)?;
        if let Some(version) = &self.version {
            validate_app_metadata_text(version, "app version", 64, false)?;
        }
        if let Some(build) = &self.build {
            validate_app_metadata_text(build, "app build", 128, false)?;
        }
        if let Some(identifier) = &self.identifier {
            validate_app_path_id(identifier).context("invalid app metadata identifier")?;
        }
        if let Some(url) = &self.website_url {
            validate_app_metadata_url(url, "app website URL")?;
        }
        if let Some(url) = &self.support_url {
            validate_app_metadata_url(url, "app support URL")?;
        }
        if let Some(copyright) = &self.copyright {
            validate_app_metadata_text(copyright, "app copyright", 256, true)?;
        }
        if let Some(license) = &self.license {
            validate_app_metadata_text(license, "app license", 256, true)?;
        }
        if let Some(credits) = &self.credits {
            validate_app_metadata_text(credits, "app credits", 2048, true)?;
        }
        Ok(())
    }

    /// Build validated metadata.
    pub fn build_checked(self) -> Result<AppMetadata> {
        self.validate()?;
        Ok(AppMetadata {
            name: self.name,
            version: self.version,
            build: self.build,
            identifier: self.identifier,
            website_url: self.website_url,
            support_url: self.support_url,
            copyright: self.copyright,
            license: self.license,
            credits: self.credits,
        })
    }
}

/// Startup context captured from the current process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContextSnapshot {
    process_id: u32,
    executable_path: Option<PathBuf>,
    current_dir: Option<PathBuf>,
    args: Vec<String>,
    environment: Vec<(String, String)>,
    debug_build: bool,
}

impl LaunchContextSnapshot {
    /// Current process id.
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Current executable path, when available.
    pub fn executable_path(&self) -> Option<&Path> {
        self.executable_path.as_deref()
    }

    /// Current working directory, when available.
    pub fn current_dir(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    /// Captured command-line arguments.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Captured environment variables from the requested allowlist.
    pub fn environment(&self) -> &[(String, String)] {
        &self.environment
    }

    /// Return an allowlisted environment value.
    pub fn env(&self, key: &str) -> Option<&str> {
        self.environment
            .iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(value.as_str()))
    }

    /// Whether the app was built with debug assertions enabled.
    pub fn is_debug_build(&self) -> bool {
        self.debug_build
    }

    /// A conservative development-mode hint for generated diagnostics and About UI.
    pub fn is_development_mode(&self) -> bool {
        self.debug_build
    }
}

/// Builder for capturing startup context without exposing arbitrary environment data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContextBuilder {
    include_args: bool,
    environment_keys: Vec<String>,
    require_executable: bool,
    require_current_dir: bool,
}

impl LaunchContextBuilder {
    /// Capture args and basic process paths, with no environment variables.
    pub fn new() -> Self {
        Self {
            include_args: true,
            environment_keys: Vec::new(),
            require_executable: false,
            require_current_dir: false,
        }
    }

    /// Do not capture command-line arguments.
    pub fn without_args(mut self) -> Self {
        self.include_args = false;
        self
    }

    /// Capture command-line arguments. This is the default.
    pub fn include_args(mut self) -> Self {
        self.include_args = true;
        self
    }

    /// Allowlist one environment variable to capture.
    pub fn environment_key(mut self, key: impl Into<String>) -> Self {
        self.environment_keys.push(key.into());
        self
    }

    /// Allowlist multiple environment variables to capture.
    pub fn environment_keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.environment_keys
            .extend(keys.into_iter().map(Into::into));
        self
    }

    /// Require the current executable path to resolve.
    pub fn require_executable(mut self) -> Self {
        self.require_executable = true;
        self
    }

    /// Require the current working directory to resolve.
    pub fn require_current_dir(mut self) -> Self {
        self.require_current_dir = true;
        self
    }

    /// Return whether command-line arguments will be captured.
    pub fn captures_args(&self) -> bool {
        self.include_args
    }

    /// Return the environment allowlist.
    pub fn environment_allowlist(&self) -> &[String] {
        &self.environment_keys
    }

    /// Validate the environment allowlist.
    pub fn validate(&self) -> Result<()> {
        for (index, key) in self.environment_keys.iter().enumerate() {
            validate_environment_key(key)?;
            anyhow::ensure!(
                !self.environment_keys[..index].contains(key),
                "launch context environment key requested more than once: {key}"
            );
        }
        Ok(())
    }

    /// Capture the validated launch context.
    pub fn capture_checked(self) -> Result<LaunchContextSnapshot> {
        self.validate()?;

        let executable_path = std::env::current_exe()
            .with_context(|| "failed to resolve launch executable path")
            .map(Some)
            .or_else(|err| {
                if self.require_executable {
                    Err(err)
                } else {
                    Ok(None)
                }
            })?;

        let current_dir = std::env::current_dir()
            .with_context(|| "failed to resolve launch current directory")
            .map(Some)
            .or_else(|err| {
                if self.require_current_dir {
                    Err(err)
                } else {
                    Ok(None)
                }
            })?;

        let args = if self.include_args {
            std::env::args_os()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        } else {
            Vec::new()
        };
        validate_launch_args(&args)?;

        let environment = self
            .environment_keys
            .into_iter()
            .filter_map(|key| std::env::var(&key).ok().map(|value| (key, value)))
            .collect::<Vec<_>>();

        for (key, value) in &environment {
            validate_environment_key(key)?;
            validate_no_nul(value, "launch context environment value")?;
        }

        Ok(LaunchContextSnapshot {
            process_id: std::process::id(),
            executable_path,
            current_dir,
            args,
            environment,
            debug_build: cfg!(debug_assertions),
        })
    }
}

impl Default for LaunchContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Text direction inferred from a locale language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleTextDirection {
    /// Left-to-right text.
    LeftToRight,
    /// Right-to-left text.
    RightToLeft,
}

/// Runtime locale and preferred-language snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleSnapshot {
    locale: String,
    language: String,
    region: Option<String>,
    preferred_languages: Vec<String>,
    text_direction: LocaleTextDirection,
    source: Option<String>,
}

impl LocaleSnapshot {
    /// Normalized primary locale tag, such as `en-US`.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Lowercase language subtag, such as `en`.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Uppercase region subtag, such as `US`, when present.
    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    /// Preferred languages in priority order.
    pub fn preferred_languages(&self) -> &[String] {
        &self.preferred_languages
    }

    /// Inferred text direction.
    pub fn text_direction(&self) -> LocaleTextDirection {
        self.text_direction
    }

    /// Whether this locale is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.text_direction == LocaleTextDirection::RightToLeft
    }

    /// Source label used to choose the primary locale, when known.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// Builder for Electron `app.getLocale()` / preferred-language style snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleSnapshotBuilder {
    candidates: Vec<(String, Option<String>)>,
    preferred_languages: Vec<String>,
    fallback_locale: String,
    use_system_environment: bool,
    require_locale: bool,
}

impl LocaleSnapshotBuilder {
    /// Create a locale snapshot builder using system environment candidates.
    pub fn new() -> Self {
        Self {
            candidates: Vec::new(),
            preferred_languages: Vec::new(),
            fallback_locale: "en-US".to_string(),
            use_system_environment: true,
            require_locale: false,
        }
    }

    /// Add an explicit locale candidate.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.candidates
            .push((locale.into(), Some("explicit".into())));
        self
    }

    /// Add an explicit locale candidate with a source label.
    pub fn locale_from(mut self, locale: impl Into<String>, source: impl Into<String>) -> Self {
        self.candidates.push((locale.into(), Some(source.into())));
        self
    }

    /// Add preferred languages in priority order.
    pub fn preferred_languages(
        mut self,
        languages: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.preferred_languages
            .extend(languages.into_iter().map(Into::into));
        self
    }

    /// Set the fallback locale used when no explicit or system locale normalizes.
    pub fn fallback_locale(mut self, locale: impl Into<String>) -> Self {
        self.fallback_locale = locale.into();
        self
    }

    /// Use system environment locale variables. This is the default.
    pub fn use_system_environment(mut self, enabled: bool) -> Self {
        self.use_system_environment = enabled;
        self
    }

    /// Require at least one non-fallback locale candidate to normalize.
    pub fn require_locale(mut self) -> Self {
        self.require_locale = true;
        self
    }

    /// Validate configured explicit values.
    pub fn validate(&self) -> Result<()> {
        normalize_locale_tag(&self.fallback_locale)
            .with_context(|| "fallback locale must be a valid locale tag")?;
        for (locale, _) in &self.candidates {
            normalize_locale_tag(locale)?;
        }
        for language in &self.preferred_languages {
            normalize_locale_tag(language)?;
        }
        Ok(())
    }

    /// Build a validated locale snapshot.
    pub fn build_checked(self) -> Result<LocaleSnapshot> {
        self.validate()?;

        let mut candidates = self.candidates;
        if self.use_system_environment {
            candidates.extend(system_locale_candidates());
        }

        let mut selected = None;
        for (candidate, source) in candidates {
            if let Some(locale) = normalize_locale_tag(&candidate)? {
                selected = Some((locale, source));
                break;
            }
        }

        anyhow::ensure!(
            selected.is_some() || !self.require_locale,
            "no locale candidate was available"
        );

        let (locale, source) = match selected {
            Some(selected) => selected,
            None => (
                normalize_locale_tag(&self.fallback_locale)?
                    .context("fallback locale normalized to empty")?,
                Some("fallback".to_string()),
            ),
        };

        let mut preferred_languages = Vec::new();
        for language in self
            .preferred_languages
            .into_iter()
            .chain(system_preferred_language_candidates())
        {
            if let Some(language) = normalize_locale_tag(&language)?
                && !preferred_languages.contains(&language)
            {
                preferred_languages.push(language);
            }
        }
        if !preferred_languages.contains(&locale) {
            preferred_languages.insert(0, locale.clone());
        }

        let language = locale
            .split('-')
            .next()
            .unwrap_or(locale.as_str())
            .to_string();
        let region = locale_region(&locale);
        let text_direction = if is_rtl_language(&language) {
            LocaleTextDirection::RightToLeft
        } else {
            LocaleTextDirection::LeftToRight
        };

        Ok(LocaleSnapshot {
            locale,
            language,
            region,
            preferred_languages,
            text_direction,
            source,
        })
    }
}

impl Default for LocaleSnapshotBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Text checking features requested for an editable text region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextCheckingRequest {
    text: String,
    locale: String,
    check_spelling: bool,
    check_grammar: bool,
    autocorrect: bool,
    custom_words: Vec<String>,
    max_suggestions: usize,
}

impl TextCheckingRequest {
    /// Start building a checked text-checking request.
    pub fn builder(text: impl Into<String>) -> TextCheckingRequestBuilder {
        TextCheckingRequestBuilder::new(text)
    }

    /// Text to check.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Normalized locale tag used for dictionary selection.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Whether spelling should be checked.
    pub fn checks_spelling(&self) -> bool {
        self.check_spelling
    }

    /// Whether grammar should be checked.
    pub fn checks_grammar(&self) -> bool {
        self.check_grammar
    }

    /// Whether autocorrect is allowed for this request.
    pub fn autocorrects(&self) -> bool {
        self.autocorrect
    }

    /// App-provided words treated as dictionary entries.
    pub fn custom_words(&self) -> &[String] {
        &self.custom_words
    }

    /// Maximum number of suggestions to return per issue.
    pub fn max_suggestions(&self) -> usize {
        self.max_suggestions
    }

    /// Validate the request shape before handing it to a spellcheck backend.
    pub fn validate(&self) -> Result<()> {
        validate_text_checking_text(&self.text)?;
        normalize_locale_tag(&self.locale)?
            .context("text checking locale must be a valid non-fallback locale")?;
        anyhow::ensure!(
            self.check_spelling || self.check_grammar || self.autocorrect,
            "text checking request must enable spelling, grammar, or autocorrect"
        );
        validate_custom_dictionary_words(&self.custom_words)?;
        anyhow::ensure!(
            self.max_suggestions <= 20,
            "text checking max suggestions cannot exceed 20"
        );
        Ok(())
    }
}

/// Builder for checked spellcheck/grammar/autocorrect requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextCheckingRequestBuilder {
    text: String,
    locale: Option<String>,
    check_spelling: bool,
    check_grammar: bool,
    autocorrect: bool,
    custom_words: Vec<String>,
    max_suggestions: usize,
}

impl TextCheckingRequestBuilder {
    /// Create a text-checking builder with spelling enabled and `en-US` fallback.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            locale: None,
            check_spelling: true,
            check_grammar: false,
            autocorrect: false,
            custom_words: Vec::new(),
            max_suggestions: 5,
        }
    }

    /// Set the dictionary locale.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// Use the locale from an existing snapshot.
    pub fn locale_snapshot(mut self, locale: &LocaleSnapshot) -> Self {
        self.locale = Some(locale.locale().to_string());
        self
    }

    /// Enable or disable spelling checks.
    pub fn check_spelling(mut self, enabled: bool) -> Self {
        self.check_spelling = enabled;
        self
    }

    /// Enable grammar checks.
    pub fn check_grammar(mut self) -> Self {
        self.check_grammar = true;
        self
    }

    /// Enable or disable grammar checks.
    pub fn check_grammar_enabled(mut self, enabled: bool) -> Self {
        self.check_grammar = enabled;
        self
    }

    /// Allow autocorrect suggestions/replacements.
    pub fn autocorrect(mut self) -> Self {
        self.autocorrect = true;
        self
    }

    /// Enable or disable autocorrect.
    pub fn autocorrect_enabled(mut self, enabled: bool) -> Self {
        self.autocorrect = enabled;
        self
    }

    /// Add one app-specific dictionary word.
    pub fn custom_word(mut self, word: impl Into<String>) -> Self {
        self.custom_words.push(word.into());
        self
    }

    /// Add app-specific dictionary words.
    pub fn custom_words(mut self, words: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.custom_words.extend(words.into_iter().map(Into::into));
        self
    }

    /// Limit suggestions returned by a backend.
    pub fn max_suggestions(mut self, max_suggestions: usize) -> Self {
        self.max_suggestions = max_suggestions;
        self
    }

    /// Validate the configured request.
    pub fn validate(&self) -> Result<()> {
        self.as_request()?.validate()
    }

    /// Build a checked text-checking request.
    pub fn build_checked(self) -> Result<TextCheckingRequest> {
        let request = self.as_request()?;
        request.validate()?;
        Ok(request)
    }

    fn as_request(&self) -> Result<TextCheckingRequest> {
        let locale = match &self.locale {
            Some(locale) => normalize_locale_tag(locale)?
                .context("text checking locale must be a valid non-fallback locale")?,
            None => "en-US".to_string(),
        };
        Ok(TextCheckingRequest {
            text: self.text.clone(),
            locale,
            check_spelling: self.check_spelling,
            check_grammar: self.check_grammar,
            autocorrect: self.autocorrect,
            custom_words: self.custom_words.clone(),
            max_suggestions: self.max_suggestions,
        })
    }
}

/// Requested location accuracy for native geolocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationAccuracy {
    /// Coarse city/region-level location.
    Coarse,
    /// Balanced accuracy suitable for nearby content.
    Balanced,
    /// High accuracy suitable for maps, routing, or precise check-ins.
    High,
}

/// A checked native location request descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationRequest {
    purpose: String,
    accuracy: LocationAccuracy,
    timeout: Duration,
    maximum_age: Duration,
    allow_background: bool,
}

impl LocationRequest {
    /// Start building a checked location request.
    pub fn builder(purpose: impl Into<String>) -> LocationRequestBuilder {
        LocationRequestBuilder::new(purpose)
    }

    /// User-facing reason for requesting location.
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Requested accuracy.
    pub fn accuracy(&self) -> LocationAccuracy {
        self.accuracy
    }

    /// Time before the request should fail or fall back.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Maximum acceptable cached location age.
    pub fn maximum_age(&self) -> Duration {
        self.maximum_age
    }

    /// Whether this request intends to continue in the background.
    pub fn allows_background(&self) -> bool {
        self.allow_background
    }

    /// Capability required to execute this request.
    pub fn required_capability(&self) -> Capability {
        Capability::Location
    }

    /// Privacy declaration needed for packaging metadata.
    pub fn privacy_permission(&self) -> AppPrivacyPermissionBuilder {
        AppPrivacyPermissionBuilder::location(self.purpose.clone())
    }

    /// Validate request shape before prompting or querying the OS.
    pub fn validate(&self) -> Result<()> {
        validate_location_purpose(&self.purpose)?;
        anyhow::ensure!(
            self.timeout > Duration::ZERO,
            "location request timeout must be greater than zero"
        );
        anyhow::ensure!(
            self.timeout <= Duration::from_secs(120),
            "location request timeout cannot exceed 120 seconds"
        );
        anyhow::ensure!(
            self.maximum_age <= Duration::from_secs(24 * 60 * 60),
            "location request maximum age cannot exceed 24 hours"
        );
        anyhow::ensure!(
            !(self.allow_background && self.accuracy == LocationAccuracy::High),
            "background location requests cannot require high accuracy"
        );
        Ok(())
    }
}

/// Builder for checked native geolocation requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationRequestBuilder {
    purpose: String,
    accuracy: LocationAccuracy,
    timeout: Duration,
    maximum_age: Duration,
    allow_background: bool,
}

impl LocationRequestBuilder {
    /// Create a location request with balanced accuracy and a short timeout.
    pub fn new(purpose: impl Into<String>) -> Self {
        Self {
            purpose: purpose.into(),
            accuracy: LocationAccuracy::Balanced,
            timeout: Duration::from_secs(10),
            maximum_age: Duration::ZERO,
            allow_background: false,
        }
    }

    /// Request coarse location.
    pub fn coarse(mut self) -> Self {
        self.accuracy = LocationAccuracy::Coarse;
        self
    }

    /// Request balanced location accuracy.
    pub fn balanced(mut self) -> Self {
        self.accuracy = LocationAccuracy::Balanced;
        self
    }

    /// Request high-accuracy location.
    pub fn high_accuracy(mut self) -> Self {
        self.accuracy = LocationAccuracy::High;
        self
    }

    /// Set the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Accept cached locations up to the given age.
    pub fn maximum_age(mut self, maximum_age: Duration) -> Self {
        self.maximum_age = maximum_age;
        self
    }

    /// Mark the request as background-capable.
    pub fn allow_background(mut self) -> Self {
        self.allow_background = true;
        self
    }

    /// Validate the configured request.
    pub fn validate(&self) -> Result<()> {
        self.as_request().validate()
    }

    /// Build a checked location request.
    pub fn build_checked(self) -> Result<LocationRequest> {
        let request = self.as_request();
        request.validate()?;
        Ok(request)
    }

    fn as_request(&self) -> LocationRequest {
        LocationRequest {
            purpose: self.purpose.clone(),
            accuracy: self.accuracy,
            timeout: self.timeout,
            maximum_age: self.maximum_age,
            allow_background: self.allow_background,
        }
    }
}

/// Native device access category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceAccessKind {
    /// USB device access similar to WebUSB.
    Usb,
    /// HID device access similar to WebHID.
    Hid,
    /// Serial port access similar to Web Serial.
    Serial,
    /// Bluetooth device/service access similar to Web Bluetooth.
    Bluetooth,
}

impl DeviceAccessKind {
    /// Capability required for this device category.
    pub fn required_capability(self) -> Capability {
        match self {
            Self::Usb => Capability::UsbDevice,
            Self::Hid => Capability::HidDevice,
            Self::Serial => Capability::SerialPort,
            Self::Bluetooth => Capability::Bluetooth,
        }
    }

    /// Privacy declaration kind for this device category.
    pub fn privacy_kind(self) -> AppPrivacyPermissionKind {
        match self {
            Self::Usb => AppPrivacyPermissionKind::UsbDevices,
            Self::Hid => AppPrivacyPermissionKind::HidDevices,
            Self::Serial => AppPrivacyPermissionKind::SerialPorts,
            Self::Bluetooth => AppPrivacyPermissionKind::Bluetooth,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Hid => "HID",
            Self::Serial => "serial",
            Self::Bluetooth => "Bluetooth",
        }
    }
}

/// A checked native device access request descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAccessRequest {
    kind: DeviceAccessKind,
    purpose: String,
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    service_uuid: Option<String>,
    port_name_hint: Option<String>,
    timeout: Duration,
    allow_background: bool,
}

impl DeviceAccessRequest {
    /// Start a USB device request.
    pub fn usb(purpose: impl Into<String>) -> DeviceAccessRequestBuilder {
        DeviceAccessRequestBuilder::usb(purpose)
    }

    /// Start a HID device request.
    pub fn hid(purpose: impl Into<String>) -> DeviceAccessRequestBuilder {
        DeviceAccessRequestBuilder::hid(purpose)
    }

    /// Start a serial port request.
    pub fn serial(purpose: impl Into<String>) -> DeviceAccessRequestBuilder {
        DeviceAccessRequestBuilder::serial(purpose)
    }

    /// Start a Bluetooth device/service request.
    pub fn bluetooth(purpose: impl Into<String>) -> DeviceAccessRequestBuilder {
        DeviceAccessRequestBuilder::bluetooth(purpose)
    }

    /// Device category.
    pub fn kind(&self) -> DeviceAccessKind {
        self.kind
    }

    /// User-facing reason for requesting device access.
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Optional vendor id filter for USB/HID discovery.
    pub fn vendor_id(&self) -> Option<u16> {
        self.vendor_id
    }

    /// Optional product id filter for USB/HID discovery.
    pub fn product_id(&self) -> Option<u16> {
        self.product_id
    }

    /// Optional Bluetooth service UUID filter.
    pub fn service_uuid(&self) -> Option<&str> {
        self.service_uuid.as_deref()
    }

    /// Optional serial-port name hint.
    pub fn port_name_hint(&self) -> Option<&str> {
        self.port_name_hint.as_deref()
    }

    /// Time before discovery/request should fail or fall back.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Whether this request intends to keep using the device in the background.
    pub fn allows_background(&self) -> bool {
        self.allow_background
    }

    /// Capability required to execute this request.
    pub fn required_capability(&self) -> Capability {
        self.kind.required_capability()
    }

    /// Privacy declaration needed for packaging metadata.
    pub fn privacy_permission(&self) -> AppPrivacyPermissionBuilder {
        AppPrivacyPermissionBuilder::new(self.kind.privacy_kind(), self.purpose.clone())
    }

    /// Validate request shape before prompting or querying the OS.
    pub fn validate(&self) -> Result<()> {
        validate_device_access_purpose(&self.purpose)?;
        anyhow::ensure!(
            self.timeout > Duration::ZERO,
            "device access request timeout must be greater than zero"
        );
        anyhow::ensure!(
            self.timeout <= Duration::from_secs(120),
            "device access request timeout cannot exceed 120 seconds"
        );
        anyhow::ensure!(
            !(self.product_id.is_some() && self.vendor_id.is_none()),
            "device access product id requires a vendor id"
        );
        match self.kind {
            DeviceAccessKind::Usb | DeviceAccessKind::Hid => {
                anyhow::ensure!(
                    self.service_uuid.is_none(),
                    "{} device requests cannot include Bluetooth service UUID filters",
                    self.kind.label()
                );
                anyhow::ensure!(
                    self.port_name_hint.is_none(),
                    "{} device requests cannot include serial port hints",
                    self.kind.label()
                );
            }
            DeviceAccessKind::Serial => {
                anyhow::ensure!(
                    self.vendor_id.is_none() && self.product_id.is_none(),
                    "serial port requests cannot include USB/HID vendor or product filters"
                );
                anyhow::ensure!(
                    self.service_uuid.is_none(),
                    "serial port requests cannot include Bluetooth service UUID filters"
                );
                if let Some(hint) = &self.port_name_hint {
                    validate_device_port_hint(hint)?;
                }
            }
            DeviceAccessKind::Bluetooth => {
                anyhow::ensure!(
                    self.vendor_id.is_none() && self.product_id.is_none(),
                    "Bluetooth requests cannot include USB/HID vendor or product filters"
                );
                anyhow::ensure!(
                    self.port_name_hint.is_none(),
                    "Bluetooth requests cannot include serial port hints"
                );
                if let Some(uuid) = &self.service_uuid {
                    validate_bluetooth_service_uuid(uuid)?;
                }
            }
        }
        Ok(())
    }
}

/// Builder for checked native device access requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAccessRequestBuilder {
    kind: DeviceAccessKind,
    purpose: String,
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    service_uuid: Option<String>,
    port_name_hint: Option<String>,
    timeout: Duration,
    allow_background: bool,
}

impl DeviceAccessRequestBuilder {
    /// Create a request for the given device category.
    pub fn new(kind: DeviceAccessKind, purpose: impl Into<String>) -> Self {
        Self {
            kind,
            purpose: purpose.into(),
            vendor_id: None,
            product_id: None,
            service_uuid: None,
            port_name_hint: None,
            timeout: Duration::from_secs(30),
            allow_background: false,
        }
    }

    /// Create a USB device request.
    pub fn usb(purpose: impl Into<String>) -> Self {
        Self::new(DeviceAccessKind::Usb, purpose)
    }

    /// Create a HID device request.
    pub fn hid(purpose: impl Into<String>) -> Self {
        Self::new(DeviceAccessKind::Hid, purpose)
    }

    /// Create a serial port request.
    pub fn serial(purpose: impl Into<String>) -> Self {
        Self::new(DeviceAccessKind::Serial, purpose)
    }

    /// Create a Bluetooth device/service request.
    pub fn bluetooth(purpose: impl Into<String>) -> Self {
        Self::new(DeviceAccessKind::Bluetooth, purpose)
    }

    /// Add a USB/HID vendor id filter.
    pub fn vendor_id(mut self, vendor_id: u16) -> Self {
        self.vendor_id = Some(vendor_id);
        self
    }

    /// Add a USB/HID product id filter.
    pub fn product_id(mut self, product_id: u16) -> Self {
        self.product_id = Some(product_id);
        self
    }

    /// Add a USB/HID vendor and product id filter.
    pub fn vendor_product(mut self, vendor_id: u16, product_id: u16) -> Self {
        self.vendor_id = Some(vendor_id);
        self.product_id = Some(product_id);
        self
    }

    /// Add a Bluetooth service UUID filter.
    pub fn service_uuid(mut self, service_uuid: impl Into<String>) -> Self {
        self.service_uuid = Some(service_uuid.into());
        self
    }

    /// Add a serial port name hint such as `tty.usbserial` or `COM3`.
    pub fn port_name_hint(mut self, port_name_hint: impl Into<String>) -> Self {
        self.port_name_hint = Some(port_name_hint.into());
        self
    }

    /// Set the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Mark the request as background-capable.
    pub fn allow_background(mut self) -> Self {
        self.allow_background = true;
        self
    }

    /// Validate the configured request.
    pub fn validate(&self) -> Result<()> {
        self.as_request().validate()
    }

    /// Build a checked device access request.
    pub fn build_checked(self) -> Result<DeviceAccessRequest> {
        let request = self.as_request();
        request.validate()?;
        Ok(request)
    }

    fn as_request(&self) -> DeviceAccessRequest {
        DeviceAccessRequest {
            kind: self.kind,
            purpose: self.purpose.clone(),
            vendor_id: self.vendor_id,
            product_id: self.product_id,
            service_uuid: self.service_uuid.clone(),
            port_name_hint: self.port_name_hint.clone(),
            timeout: self.timeout,
            allow_background: self.allow_background,
        }
    }
}

/// Best-effort memory information for the current Kael process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMemoryMetrics {
    resident_set_bytes: Option<u64>,
    virtual_memory_bytes: Option<u64>,
    source: Option<&'static str>,
}

impl ProcessMemoryMetrics {
    /// Create an empty memory metrics snapshot for unsupported platforms.
    pub fn unsupported() -> Self {
        Self {
            resident_set_bytes: None,
            virtual_memory_bytes: None,
            source: None,
        }
    }

    /// Resident set size in bytes, when available.
    pub fn resident_set_bytes(&self) -> Option<u64> {
        self.resident_set_bytes
    }

    /// Virtual memory size in bytes, when available.
    pub fn virtual_memory_bytes(&self) -> Option<u64> {
        self.virtual_memory_bytes
    }

    /// Source used for the memory sample, such as `/proc/self/statm` or `ps`.
    pub fn source(&self) -> Option<&'static str> {
        self.source
    }

    /// Whether at least one memory value was sampled.
    pub fn is_supported(&self) -> bool {
        self.resident_set_bytes.is_some() || self.virtual_memory_bytes.is_some()
    }
}

/// Runtime metrics for the current Kael application process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMetricsSnapshot {
    process_id: u32,
    executable_path: Option<PathBuf>,
    current_dir: Option<PathBuf>,
    uptime: Duration,
    window_count: usize,
    memory: ProcessMemoryMetrics,
}

impl ProcessMetricsSnapshot {
    /// Current process id.
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Current executable path, when the OS exposes it.
    pub fn executable_path(&self) -> Option<&Path> {
        self.executable_path.as_deref()
    }

    /// Current working directory, when available.
    pub fn current_dir(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    /// Duration since the Kael app state was created.
    pub fn uptime(&self) -> Duration {
        self.uptime
    }

    /// Number of open Kael windows.
    pub fn window_count(&self) -> usize {
        self.window_count
    }

    /// Best-effort memory metrics.
    pub fn memory(&self) -> &ProcessMemoryMetrics {
        &self.memory
    }

    /// Resident set size in bytes, when available.
    pub fn resident_set_bytes(&self) -> Option<u64> {
        self.memory.resident_set_bytes()
    }

    /// Virtual memory size in bytes, when available.
    pub fn virtual_memory_bytes(&self) -> Option<u64> {
        self.memory.virtual_memory_bytes()
    }
}

/// Validated resource budget for a lightweight Kael app runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppResourceBudget {
    max_resident_set_bytes: Option<u64>,
    max_virtual_memory_bytes: Option<u64>,
    max_windows: Option<usize>,
    max_uptime: Option<Duration>,
    require_memory_metrics: bool,
    warn_when_power_constrained: bool,
}

impl AppResourceBudget {
    /// Resident set budget in bytes, when configured.
    pub fn max_resident_set_bytes(&self) -> Option<u64> {
        self.max_resident_set_bytes
    }

    /// Virtual memory budget in bytes, when configured.
    pub fn max_virtual_memory_bytes(&self) -> Option<u64> {
        self.max_virtual_memory_bytes
    }

    /// Maximum open-window count, when configured.
    pub fn max_windows(&self) -> Option<usize> {
        self.max_windows
    }

    /// Maximum process uptime, when configured.
    pub fn max_uptime(&self) -> Option<Duration> {
        self.max_uptime
    }

    /// Whether missing memory metrics are treated as a budget issue.
    pub fn requires_memory_metrics(&self) -> bool {
        self.require_memory_metrics
    }

    /// Whether low-power/reduce-motion state should produce a warning issue.
    pub fn warns_when_power_constrained(&self) -> bool {
        self.warn_when_power_constrained
    }
}

/// Builder for checked resource budgets over current app process state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppResourceBudgetBuilder {
    max_resident_set_bytes: Option<u64>,
    max_virtual_memory_bytes: Option<u64>,
    max_windows: Option<usize>,
    max_uptime: Option<Duration>,
    require_memory_metrics: bool,
    warn_when_power_constrained: bool,
}

impl AppResourceBudgetBuilder {
    /// Create an empty budget builder.
    pub fn new() -> Self {
        Self {
            max_resident_set_bytes: None,
            max_virtual_memory_bytes: None,
            max_windows: None,
            max_uptime: None,
            require_memory_metrics: false,
            warn_when_power_constrained: false,
        }
    }

    /// Limit resident set size in bytes.
    pub fn max_resident_set_bytes(mut self, bytes: u64) -> Self {
        self.max_resident_set_bytes = Some(bytes);
        self
    }

    /// Limit virtual memory size in bytes.
    pub fn max_virtual_memory_bytes(mut self, bytes: u64) -> Self {
        self.max_virtual_memory_bytes = Some(bytes);
        self
    }

    /// Limit the number of open windows.
    pub fn max_windows(mut self, count: usize) -> Self {
        self.max_windows = Some(count);
        self
    }

    /// Limit process uptime.
    pub fn max_uptime(mut self, uptime: Duration) -> Self {
        self.max_uptime = Some(uptime);
        self
    }

    /// Treat unsupported memory metrics as a budget issue.
    pub fn require_memory_metrics(mut self) -> Self {
        self.require_memory_metrics = true;
        self
    }

    /// Warn when system power/accessibility state recommends reducing work.
    pub fn warn_when_power_constrained(mut self) -> Self {
        self.warn_when_power_constrained = true;
        self
    }

    /// Validate configured thresholds.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.max_resident_set_bytes.is_some()
                || self.max_virtual_memory_bytes.is_some()
                || self.max_windows.is_some()
                || self.max_uptime.is_some()
                || self.require_memory_metrics
                || self.warn_when_power_constrained,
            "resource budget must configure at least one check"
        );
        if let Some(bytes) = self.max_resident_set_bytes {
            anyhow::ensure!(bytes > 0, "resident set budget must be greater than zero");
        }
        if let Some(bytes) = self.max_virtual_memory_bytes {
            anyhow::ensure!(bytes > 0, "virtual memory budget must be greater than zero");
        }
        if let Some(count) = self.max_windows {
            anyhow::ensure!(count > 0, "window budget must be greater than zero");
        }
        if let Some(uptime) = self.max_uptime {
            anyhow::ensure!(
                uptime > Duration::ZERO,
                "uptime budget must be greater than zero"
            );
        }
        Ok(())
    }

    /// Build a checked resource budget.
    pub fn build_checked(self) -> Result<AppResourceBudget> {
        self.validate()?;
        Ok(AppResourceBudget {
            max_resident_set_bytes: self.max_resident_set_bytes,
            max_virtual_memory_bytes: self.max_virtual_memory_bytes,
            max_windows: self.max_windows,
            max_uptime: self.max_uptime,
            require_memory_metrics: self.require_memory_metrics,
            warn_when_power_constrained: self.warn_when_power_constrained,
        })
    }
}

impl Default for AppResourceBudgetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Kind of resource budget issue found in a process snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppResourceBudgetIssueKind {
    /// The platform did not expose required memory metrics.
    MissingMemoryMetrics,
    /// Resident set size exceeded the configured limit.
    ResidentSetExceeded,
    /// Virtual memory size exceeded the configured limit.
    VirtualMemoryExceeded,
    /// Open-window count exceeded the configured limit.
    WindowCountExceeded,
    /// Process uptime exceeded the configured limit.
    UptimeExceeded,
    /// Power/accessibility state recommends reducing work.
    PowerConstrained,
}

/// A single resource budget issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppResourceBudgetIssue {
    kind: AppResourceBudgetIssueKind,
    message: String,
}

impl AppResourceBudgetIssue {
    /// Issue kind.
    pub fn kind(&self) -> AppResourceBudgetIssueKind {
        self.kind
    }

    /// Human-readable issue summary.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Result of evaluating a resource budget against current app state.
#[derive(Debug, Clone)]
pub struct AppResourceBudgetEvaluation {
    budget: AppResourceBudget,
    metrics: ProcessMetricsSnapshot,
    runtime: AppRuntimeSnapshot,
    issues: Vec<AppResourceBudgetIssue>,
}

impl AppResourceBudgetEvaluation {
    /// Budget used for this evaluation.
    pub fn budget(&self) -> &AppResourceBudget {
        &self.budget
    }

    /// Process metrics sampled for this evaluation.
    pub fn metrics(&self) -> &ProcessMetricsSnapshot {
        &self.metrics
    }

    /// Runtime state sampled for this evaluation.
    pub fn runtime(&self) -> &AppRuntimeSnapshot {
        &self.runtime
    }

    /// Budget issues found during evaluation.
    pub fn issues(&self) -> &[AppResourceBudgetIssue] {
        &self.issues
    }

    /// Whether all configured budget checks passed.
    pub fn is_within_budget(&self) -> bool {
        self.issues.is_empty()
    }

    /// Whether required memory metrics were unavailable.
    pub fn missing_required_metrics(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.kind == AppResourceBudgetIssueKind::MissingMemoryMetrics)
    }

    /// Compact summary for logs, diagnostics, or agent responses.
    pub fn summary(&self) -> String {
        if self.is_within_budget() {
            "resource budget ok".to_string()
        } else {
            self.issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

/// Privacy-aware support diagnostics collected from native app state.
#[derive(Debug, Clone)]
pub struct SupportDiagnosticsSnapshot {
    metadata: Option<AppMetadata>,
    launch: LaunchContextSnapshot,
    locale: LocaleSnapshot,
    process: ProcessMetricsSnapshot,
    os: OsInfo,
    app_paths: Option<AppPathSet>,
    included_environment_keys: Vec<String>,
}

impl SupportDiagnosticsSnapshot {
    /// App identity metadata included in the diagnostics, when configured.
    pub fn metadata(&self) -> Option<&AppMetadata> {
        self.metadata.as_ref()
    }

    /// Startup context captured for the diagnostics.
    pub fn launch(&self) -> &LaunchContextSnapshot {
        &self.launch
    }

    /// Locale snapshot included in the diagnostics.
    pub fn locale(&self) -> &LocaleSnapshot {
        &self.locale
    }

    /// Current process metrics included in the diagnostics.
    pub fn process(&self) -> &ProcessMetricsSnapshot {
        &self.process
    }

    /// Operating-system information included in the diagnostics.
    pub fn os(&self) -> &OsInfo {
        &self.os
    }

    /// Resolved app paths included in the diagnostics, when configured.
    pub fn app_paths(&self) -> Option<&AppPathSet> {
        self.app_paths.as_ref()
    }

    /// Environment keys requested by the diagnostics builder.
    pub fn included_environment_keys(&self) -> &[String] {
        &self.included_environment_keys
    }

    /// Render a compact copy-paste support report.
    pub fn to_text(&self) -> String {
        let mut text = String::from("Kael diagnostics\n");
        if let Some(metadata) = &self.metadata {
            let _ = writeln!(text, "App: {}", metadata.display_title());
            if let Some(identifier) = metadata.identifier() {
                let _ = writeln!(text, "Identifier: {identifier}");
            }
            if let Some(build) = metadata.build() {
                let _ = writeln!(text, "Build: {build}");
            }
        }

        let _ = writeln!(
            text,
            "OS: {} {} ({})",
            self.os.name, self.os.version, self.os.arch
        );
        let _ = writeln!(
            text,
            "Locale: {} ({:?})",
            self.locale.locale(),
            self.locale.text_direction()
        );
        let _ = writeln!(text, "Process: {}", self.process.process_id());
        let _ = writeln!(text, "Windows: {}", self.process.window_count());
        let _ = writeln!(text, "Uptime ms: {}", self.process.uptime().as_millis());
        let _ = writeln!(
            text,
            "Resident set bytes: {}",
            optional_u64_label(self.process.resident_set_bytes())
        );
        let _ = writeln!(
            text,
            "Virtual memory bytes: {}",
            optional_u64_label(self.process.virtual_memory_bytes())
        );
        if let Some(path) = self.launch.executable_path() {
            let _ = writeln!(text, "Executable: {}", path.display());
        }
        if let Some(path) = self.launch.current_dir() {
            let _ = writeln!(text, "Current dir: {}", path.display());
        }
        let _ = writeln!(text, "Args captured: {}", self.launch.args().len());
        let _ = writeln!(
            text,
            "Environment keys: {}",
            if self.included_environment_keys.is_empty() {
                "none".to_string()
            } else {
                self.included_environment_keys.join(", ")
            }
        );

        if let Some(paths) = &self.app_paths {
            let _ = writeln!(text, "App paths: {}", paths.app_id());
            for (role, path) in paths.paths() {
                let _ = writeln!(text, "  {}: {}", role.key(), path.display());
            }
        }

        text
    }
}

/// Builder for support diagnostics that are safe to copy into bug reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportDiagnosticsBuilder {
    metadata: Option<AppMetadataBuilder>,
    launch: LaunchContextBuilder,
    locale: Option<LocaleSnapshotBuilder>,
    app_paths: Option<AppPathBuilder>,
}

impl SupportDiagnosticsBuilder {
    /// Create diagnostics with privacy-safe defaults.
    pub fn new() -> Self {
        Self {
            metadata: None,
            launch: LaunchContextBuilder::new().without_args(),
            locale: None,
            app_paths: None,
        }
    }

    /// Include validated app identity metadata.
    pub fn metadata(mut self, metadata: AppMetadataBuilder) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Include command-line arguments. Off by default for support reports.
    pub fn include_launch_args(mut self) -> Self {
        self.launch = self.launch.include_args();
        self
    }

    /// Allowlist one environment variable to capture in the launch snapshot.
    pub fn environment_key(mut self, key: impl Into<String>) -> Self {
        self.launch = self.launch.environment_key(key);
        self
    }

    /// Allowlist multiple environment variables to capture in the launch snapshot.
    pub fn environment_keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.launch = self.launch.environment_keys(keys);
        self
    }

    /// Override the launch-context policy.
    pub fn launch_context(mut self, launch: LaunchContextBuilder) -> Self {
        self.launch = launch;
        self
    }

    /// Override the locale snapshot policy.
    pub fn locale(mut self, locale: LocaleSnapshotBuilder) -> Self {
        self.locale = Some(locale);
        self
    }

    /// Include resolved app paths. Diagnostics never create missing directories.
    pub fn app_paths(mut self, paths: AppPathBuilder) -> Self {
        self.app_paths = Some(paths);
        self
    }

    /// Validate every configured diagnostics source.
    pub fn validate(&self) -> Result<()> {
        if let Some(metadata) = &self.metadata {
            metadata.validate()?;
        }
        self.launch.validate()?;
        if let Some(locale) = &self.locale {
            locale.validate()?;
        }
        if let Some(paths) = &self.app_paths {
            paths.validate()?;
            anyhow::ensure!(
                !paths.creates_dirs(),
                "support diagnostics app paths must not create directories"
            );
        }
        Ok(())
    }

    /// Collect a validated support diagnostics snapshot.
    pub fn build_checked(self, app: &App) -> Result<SupportDiagnosticsSnapshot> {
        self.validate()?;

        let metadata = self
            .metadata
            .map(AppMetadataBuilder::build_checked)
            .transpose()?;
        let included_environment_keys = self.launch.environment_allowlist().to_vec();
        let launch = app.launch_context_checked(self.launch)?;
        let locale = match self.locale {
            Some(locale) => app.locale_snapshot_checked(locale)?,
            None => app.locale_snapshot(),
        };
        let process = app.current_process_metrics();
        let os = app.os_info();
        let app_paths = self
            .app_paths
            .map(|paths| app.app_paths_checked(paths))
            .transpose()?;

        Ok(SupportDiagnosticsSnapshot {
            metadata,
            launch,
            locale,
            process,
            os,
            app_paths,
            included_environment_keys,
        })
    }
}

impl Default for SupportDiagnosticsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Release channel used by app update UI and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppUpdateChannel {
    /// Stable production releases.
    Stable,
    /// Beta/pre-release channel.
    Beta,
    /// Nightly or development channel.
    Dev,
    /// App-defined channel label.
    Custom(String),
}

impl AppUpdateChannel {
    /// Create a custom channel label.
    pub fn custom(channel: impl Into<String>) -> Self {
        Self::Custom(channel.into())
    }

    /// User-facing channel label.
    pub fn label(&self) -> &str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Dev => "dev",
            Self::Custom(channel) => channel,
        }
    }

    /// Validate channel labels before they reach settings or diagnostics.
    pub fn validate(&self) -> Result<()> {
        if let Self::Custom(channel) = self {
            validate_app_metadata_text(channel, "update channel", 64, false)?;
        }
        Ok(())
    }
}

/// Metadata for an app update discovered by a feed, service, or custom backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateRelease {
    version: String,
    channel: Option<AppUpdateChannel>,
    title: Option<String>,
    notes: Option<String>,
    notes_url: Option<String>,
    download_url: Option<String>,
    critical: bool,
    mandatory: bool,
    signed: bool,
    rollout_percentage: Option<u8>,
}

impl AppUpdateRelease {
    /// Version string advertised by the update source.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Optional channel this release belongs to.
    pub fn channel(&self) -> Option<&AppUpdateChannel> {
        self.channel.as_ref()
    }

    /// Optional release title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Optional short release notes.
    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    /// Optional URL for full release notes.
    pub fn notes_url(&self) -> Option<&str> {
        self.notes_url.as_deref()
    }

    /// Optional package/download URL.
    pub fn download_url(&self) -> Option<&str> {
        self.download_url.as_deref()
    }

    /// Whether this update should be presented as urgent.
    pub fn is_critical(&self) -> bool {
        self.critical
    }

    /// Whether this update must be installed once accepted.
    pub fn is_mandatory(&self) -> bool {
        self.mandatory
    }

    /// Whether this release has already passed feed/package signature checks.
    pub fn is_signed(&self) -> bool {
        self.signed
    }

    /// Percentage of clients eligible for this release, when rollout-limited.
    pub fn rollout_percentage(&self) -> Option<u8> {
        self.rollout_percentage
    }

    /// Display label such as `Version 1.2.3`.
    pub fn display_title(&self) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| format!("Version {}", self.version))
    }
}

/// Builder for checked update release metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateReleaseBuilder {
    version: String,
    channel: Option<AppUpdateChannel>,
    title: Option<String>,
    notes: Option<String>,
    notes_url: Option<String>,
    download_url: Option<String>,
    critical: bool,
    mandatory: bool,
    signed: bool,
    rollout_percentage: Option<u8>,
}

impl AppUpdateReleaseBuilder {
    /// Create release metadata with a version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            channel: None,
            title: None,
            notes: None,
            notes_url: None,
            download_url: None,
            critical: false,
            mandatory: false,
            signed: false,
            rollout_percentage: None,
        }
    }

    /// Set the release channel advertised by the feed.
    pub fn channel(mut self, channel: AppUpdateChannel) -> Self {
        self.channel = Some(channel);
        self
    }

    /// Set a short release title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set short release notes for app chrome.
    pub fn notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }

    /// Set a URL for full release notes.
    pub fn notes_url(mut self, url: impl Into<String>) -> Self {
        self.notes_url = Some(url.into());
        self
    }

    /// Set a URL for the update package or download page.
    pub fn download_url(mut self, url: impl Into<String>) -> Self {
        self.download_url = Some(url.into());
        self
    }

    /// Mark this update as critical.
    pub fn critical(mut self) -> Self {
        self.critical = true;
        self
    }

    /// Mark this update as mandatory once accepted.
    pub fn mandatory(mut self) -> Self {
        self.mandatory = true;
        self
    }

    /// Mark this release as having passed signature verification.
    pub fn signed(mut self) -> Self {
        self.signed = true;
        self
    }

    /// Limit this release to a rollout percentage in the range `0..=100`.
    pub fn rollout_percentage(mut self, percentage: u8) -> Self {
        self.rollout_percentage = Some(percentage);
        self
    }

    /// Validate release metadata.
    pub fn validate(&self) -> Result<()> {
        validate_app_metadata_text(&self.version, "update version", 64, false)?;
        if let Some(channel) = &self.channel {
            channel.validate()?;
        }
        if let Some(title) = &self.title {
            validate_app_metadata_text(title, "update title", 128, false)?;
        }
        if let Some(notes) = &self.notes {
            validate_app_metadata_text(notes, "update notes", 4096, true)?;
        }
        if let Some(url) = &self.notes_url {
            validate_app_metadata_url(url, "update notes URL")?;
        }
        if let Some(url) = &self.download_url {
            validate_app_metadata_url(url, "update download URL")?;
        }
        if let Some(percentage) = self.rollout_percentage {
            anyhow::ensure!(
                percentage <= 100,
                "update rollout percentage must be between 0 and 100"
            );
        }
        Ok(())
    }

    /// Build checked release metadata.
    pub fn build_checked(self) -> Result<AppUpdateRelease> {
        self.validate()?;
        Ok(AppUpdateRelease {
            version: self.version,
            channel: self.channel,
            title: self.title,
            notes: self.notes,
            notes_url: self.notes_url,
            download_url: self.download_url,
            critical: self.critical,
            mandatory: self.mandatory,
            signed: self.signed,
            rollout_percentage: self.rollout_percentage,
        })
    }
}

/// Result of applying app update policy to one discovered release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppUpdateOfferKind {
    /// The app can present and act on this update.
    Offer,
    /// The app should hide this update for now, usually because of channel or rollout.
    Defer,
    /// The app should reject this update because it violates local policy.
    Block,
}

/// Reason an update was offered, deferred, or blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppUpdateOfferReason {
    /// Release passed policy checks.
    Eligible,
    /// Release belongs to a different channel.
    ChannelMismatch,
    /// This client is outside the rollout bucket.
    RolloutExcluded,
    /// Policy requires a package URL but the release only has notes/metadata.
    MissingDownloadUrl,
    /// Policy requires a signed release but signature verification has not passed.
    UnsignedRelease,
}

/// Checked decision for one update release under app policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateOfferDecision {
    kind: AppUpdateOfferKind,
    reason: AppUpdateOfferReason,
    mandatory: bool,
}

impl AppUpdateOfferDecision {
    /// Decision kind.
    pub fn kind(&self) -> AppUpdateOfferKind {
        self.kind
    }

    /// Policy reason for this decision.
    pub fn reason(&self) -> AppUpdateOfferReason {
        self.reason
    }

    /// Whether this release is mandatory once accepted.
    pub fn is_mandatory(&self) -> bool {
        self.mandatory
    }

    /// Whether the app can present this release as available.
    pub fn should_offer(&self) -> bool {
        self.kind == AppUpdateOfferKind::Offer
    }

    /// Whether the app should reject this release rather than silently hide it.
    pub fn is_blocked(&self) -> bool {
        self.kind == AppUpdateOfferKind::Block
    }
}

/// Checked app-facing update policy for channel, signing, download, and rollout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateOfferPolicy {
    channel: AppUpdateChannel,
    require_signed_release: bool,
    require_download_url: bool,
    rollout_bucket: Option<u8>,
    cohort_key: Option<String>,
    allow_critical_bypass_rollout: bool,
}

impl AppUpdateOfferPolicy {
    /// Channel this app should track.
    pub fn channel(&self) -> &AppUpdateChannel {
        &self.channel
    }

    /// Whether unsigned releases are blocked.
    pub fn require_signed_release(&self) -> bool {
        self.require_signed_release
    }

    /// Whether releases without download/package URLs are blocked.
    pub fn require_download_url(&self) -> bool {
        self.require_download_url
    }

    /// Explicit rollout bucket in the range `0..=99`, when configured.
    pub fn rollout_bucket(&self) -> Option<u8> {
        self.rollout_bucket
    }

    /// Stable cohort key used to derive a rollout bucket, when configured.
    pub fn cohort_key(&self) -> Option<&str> {
        self.cohort_key.as_deref()
    }

    /// Whether critical or mandatory updates bypass rollout throttles.
    pub fn allow_critical_bypass_rollout(&self) -> bool {
        self.allow_critical_bypass_rollout
    }

    /// Apply policy to one checked release.
    pub fn evaluate_release(&self, release: &AppUpdateRelease) -> AppUpdateOfferDecision {
        if release
            .channel()
            .is_some_and(|channel| channel.label() != self.channel.label())
        {
            return update_offer_decision(
                AppUpdateOfferKind::Defer,
                AppUpdateOfferReason::ChannelMismatch,
                release,
            );
        }

        if self.require_download_url && release.download_url().is_none() {
            return update_offer_decision(
                AppUpdateOfferKind::Block,
                AppUpdateOfferReason::MissingDownloadUrl,
                release,
            );
        }

        if self.require_signed_release && !release.is_signed() {
            return update_offer_decision(
                AppUpdateOfferKind::Block,
                AppUpdateOfferReason::UnsignedRelease,
                release,
            );
        }

        let bypass_rollout =
            self.allow_critical_bypass_rollout && (release.is_critical() || release.is_mandatory());
        if !bypass_rollout {
            if let Some(percentage) = release.rollout_percentage() {
                let bucket = self
                    .rollout_bucket
                    .or_else(|| self.cohort_key.as_deref().map(update_rollout_bucket))
                    .unwrap_or(0);
                if bucket >= percentage {
                    return update_offer_decision(
                        AppUpdateOfferKind::Defer,
                        AppUpdateOfferReason::RolloutExcluded,
                        release,
                    );
                }
            }
        }

        update_offer_decision(
            AppUpdateOfferKind::Offer,
            AppUpdateOfferReason::Eligible,
            release,
        )
    }
}

/// Builder for checked app update offer policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateOfferPolicyBuilder {
    channel: AppUpdateChannel,
    require_signed_release: bool,
    require_download_url: bool,
    rollout_bucket: Option<u8>,
    cohort_key: Option<String>,
    allow_critical_bypass_rollout: bool,
}

impl AppUpdateOfferPolicyBuilder {
    /// Create a policy for a release channel.
    pub fn new(channel: AppUpdateChannel) -> Self {
        Self {
            channel,
            require_signed_release: true,
            require_download_url: true,
            rollout_bucket: None,
            cohort_key: None,
            allow_critical_bypass_rollout: true,
        }
    }

    /// Conservative stable-channel policy.
    pub fn stable() -> Self {
        Self::new(AppUpdateChannel::Stable)
    }

    /// Require or allow unsigned releases.
    pub fn require_signed_release(mut self, require: bool) -> Self {
        self.require_signed_release = require;
        self
    }

    /// Allow unsigned releases for local testing or externally verified feeds.
    pub fn allow_unsigned_release(mut self) -> Self {
        self.require_signed_release = false;
        self
    }

    /// Require or allow releases without package URLs.
    pub fn require_download_url(mut self, require: bool) -> Self {
        self.require_download_url = require;
        self
    }

    /// Allow release-notes-only update announcements.
    pub fn allow_release_notes_only(mut self) -> Self {
        self.require_download_url = false;
        self
    }

    /// Set an explicit rollout bucket in the range `0..=99`.
    pub fn rollout_bucket(mut self, bucket: u8) -> Self {
        self.rollout_bucket = Some(bucket);
        self
    }

    /// Set a stable cohort key that derives a rollout bucket.
    pub fn cohort_key(mut self, key: impl Into<String>) -> Self {
        self.cohort_key = Some(key.into());
        self
    }

    /// Control whether critical/mandatory releases bypass rollout throttles.
    pub fn allow_critical_bypass_rollout(mut self, allow: bool) -> Self {
        self.allow_critical_bypass_rollout = allow;
        self
    }

    /// Validate this policy.
    pub fn validate(&self) -> Result<()> {
        self.channel.validate()?;
        if let Some(bucket) = self.rollout_bucket {
            anyhow::ensure!(
                bucket < 100,
                "update rollout bucket must be between 0 and 99"
            );
        }
        if let Some(cohort_key) = &self.cohort_key {
            validate_app_metadata_text(cohort_key, "update cohort key", 256, false)?;
        }
        anyhow::ensure!(
            !(self.rollout_bucket.is_some() && self.cohort_key.is_some()),
            "configure either update rollout bucket or cohort key, not both"
        );
        Ok(())
    }

    /// Build a checked update offer policy.
    pub fn build_checked(self) -> Result<AppUpdateOfferPolicy> {
        self.validate()?;
        Ok(AppUpdateOfferPolicy {
            channel: self.channel,
            require_signed_release: self.require_signed_release,
            require_download_url: self.require_download_url,
            rollout_bucket: self.rollout_bucket,
            cohort_key: self.cohort_key,
            allow_critical_bypass_rollout: self.allow_critical_bypass_rollout,
        })
    }
}

fn update_offer_decision(
    kind: AppUpdateOfferKind,
    reason: AppUpdateOfferReason,
    release: &AppUpdateRelease,
) -> AppUpdateOfferDecision {
    AppUpdateOfferDecision {
        kind,
        reason,
        mandatory: release.is_mandatory(),
    }
}

fn update_rollout_bucket(cohort_key: &str) -> u8 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in cohort_key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % 100) as u8
}

/// Current state of an app update flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppUpdatePhase {
    /// No update check is active.
    Idle,
    /// The app is checking for updates.
    Checking,
    /// The current version is up to date.
    UpToDate,
    /// A newer release is available.
    Available,
    /// An update package is downloading.
    Downloading,
    /// The update package finished downloading.
    Downloaded,
    /// The update is staged and ready to install on restart.
    ReadyToInstall,
    /// The most recent update operation failed.
    Failed,
}

/// Recommended UI action for the current update state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppUpdateAction {
    /// Offer a manual update check.
    CheckNow,
    /// Show passive progress only.
    Wait,
    /// Offer to download the available update.
    Download,
    /// Offer to install or restart into the downloaded update.
    RestartToInstall,
    /// Offer to open release notes.
    OpenReleaseNotes,
    /// Offer a retry after failure.
    Retry,
}

/// Checked update state for menus, settings pages, notifications, and agents.
#[derive(Debug, Clone, PartialEq)]
pub struct AppUpdateState {
    current_version: String,
    channel: AppUpdateChannel,
    phase: AppUpdatePhase,
    release: Option<AppUpdateRelease>,
    download_progress: Option<f32>,
    error_message: Option<String>,
}

impl AppUpdateState {
    /// Current installed app version.
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Configured release channel.
    pub fn channel(&self) -> &AppUpdateChannel {
        &self.channel
    }

    /// Current update phase.
    pub fn phase(&self) -> AppUpdatePhase {
        self.phase
    }

    /// Available or downloaded release metadata, when present.
    pub fn release(&self) -> Option<&AppUpdateRelease> {
        self.release.as_ref()
    }

    /// Download progress in the range `0.0..=1.0`, when downloading.
    pub fn download_progress(&self) -> Option<f32> {
        self.download_progress
    }

    /// Last update error, when the phase is `Failed`.
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    /// Whether a discovered release differs from the current version.
    pub fn has_update(&self) -> bool {
        self.release
            .as_ref()
            .is_some_and(|release| release.version() != self.current_version)
    }

    /// Recommended primary UI action for this update state.
    pub fn recommended_action(&self) -> AppUpdateAction {
        match self.phase {
            AppUpdatePhase::Idle | AppUpdatePhase::UpToDate => AppUpdateAction::CheckNow,
            AppUpdatePhase::Checking | AppUpdatePhase::Downloading => AppUpdateAction::Wait,
            AppUpdatePhase::Available => {
                if self
                    .release
                    .as_ref()
                    .and_then(AppUpdateRelease::download_url)
                    .is_some()
                {
                    AppUpdateAction::Download
                } else {
                    AppUpdateAction::OpenReleaseNotes
                }
            }
            AppUpdatePhase::Downloaded | AppUpdatePhase::ReadyToInstall => {
                AppUpdateAction::RestartToInstall
            }
            AppUpdatePhase::Failed => AppUpdateAction::Retry,
        }
    }

    /// Short label suitable for a menu item or status row.
    pub fn menu_label(&self) -> String {
        match self.phase {
            AppUpdatePhase::Idle => "Check for Updates...".to_string(),
            AppUpdatePhase::Checking => "Checking for Updates...".to_string(),
            AppUpdatePhase::UpToDate => "Up to Date".to_string(),
            AppUpdatePhase::Available => self
                .release
                .as_ref()
                .map(|release| format!("Download {}", release.display_title()))
                .unwrap_or_else(|| "Update Available".to_string()),
            AppUpdatePhase::Downloading => match self.download_progress {
                Some(progress) => format!("Downloading Update {:.0}%", progress * 100.0),
                None => "Downloading Update...".to_string(),
            },
            AppUpdatePhase::Downloaded | AppUpdatePhase::ReadyToInstall => {
                "Restart to Install Update".to_string()
            }
            AppUpdatePhase::Failed => "Retry Update Check".to_string(),
        }
    }
}

/// Builder for checked app update state.
#[derive(Debug, Clone, PartialEq)]
pub struct AppUpdateStateBuilder {
    current_version: String,
    channel: AppUpdateChannel,
    phase: AppUpdatePhase,
    release: Option<AppUpdateReleaseBuilder>,
    download_progress: Option<f32>,
    error_message: Option<String>,
}

impl AppUpdateStateBuilder {
    /// Create update state for the current app version.
    pub fn new(current_version: impl Into<String>) -> Self {
        Self {
            current_version: current_version.into(),
            channel: AppUpdateChannel::Stable,
            phase: AppUpdatePhase::Idle,
            release: None,
            download_progress: None,
            error_message: None,
        }
    }

    /// Set the release channel.
    pub fn channel(mut self, channel: AppUpdateChannel) -> Self {
        self.channel = channel;
        self
    }

    /// Set the update phase.
    pub fn phase(mut self, phase: AppUpdatePhase) -> Self {
        self.phase = phase;
        self
    }

    /// Set release metadata for available/downloaded states.
    pub fn release(mut self, release: AppUpdateReleaseBuilder) -> Self {
        self.release = Some(release);
        self
    }

    /// Set download progress in the range `0.0..=1.0`.
    pub fn download_progress(mut self, progress: f32) -> Self {
        self.download_progress = Some(progress);
        self
    }

    /// Set the last update error.
    pub fn error_message(mut self, message: impl Into<String>) -> Self {
        self.error_message = Some(message.into());
        self
    }

    /// Validate the update state model.
    pub fn validate(&self) -> Result<()> {
        validate_app_metadata_text(&self.current_version, "current update version", 64, false)?;
        self.channel.validate()?;
        if let Some(release) = &self.release {
            release.validate()?;
        }
        if let Some(progress) = self.download_progress {
            anyhow::ensure!(
                progress.is_finite() && (0.0..=1.0).contains(&progress),
                "update download progress must be between 0.0 and 1.0"
            );
        }
        if let Some(message) = &self.error_message {
            validate_app_metadata_text(message, "update error message", 512, true)?;
        }

        match self.phase {
            AppUpdatePhase::Available
            | AppUpdatePhase::Downloading
            | AppUpdatePhase::Downloaded
            | AppUpdatePhase::ReadyToInstall => {
                anyhow::ensure!(
                    self.release.is_some(),
                    "update phase {:?} requires release metadata",
                    self.phase
                );
            }
            AppUpdatePhase::Failed => {
                anyhow::ensure!(
                    self.error_message.is_some(),
                    "failed update state requires an error message"
                );
            }
            AppUpdatePhase::Idle | AppUpdatePhase::Checking | AppUpdatePhase::UpToDate => {}
        }

        anyhow::ensure!(
            self.download_progress.is_none() || self.phase == AppUpdatePhase::Downloading,
            "download progress is only valid while downloading"
        );
        anyhow::ensure!(
            self.error_message.is_none() || self.phase == AppUpdatePhase::Failed,
            "update error messages are only valid for failed state"
        );

        Ok(())
    }

    /// Build checked update state.
    pub fn build_checked(self) -> Result<AppUpdateState> {
        self.validate()?;
        Ok(AppUpdateState {
            current_version: self.current_version,
            channel: self.channel,
            phase: self.phase,
            release: self
                .release
                .map(AppUpdateReleaseBuilder::build_checked)
                .transpose()?,
            download_progress: self.download_progress,
            error_message: self.error_message,
        })
    }
}

/// Builder for a restart binary path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPathBuilder {
    path: PathBuf,
    require_existing_file: bool,
    canonicalize_path: bool,
}

impl RestartPathBuilder {
    /// Create a restart path builder for a custom relaunch binary.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            require_existing_file: true,
            canonicalize_path: false,
        }
    }

    /// Create a restart path builder from the current executable.
    pub fn current_exe() -> Result<Self> {
        Ok(Self::new(
            std::env::current_exe().context("failed to resolve current executable")?,
        ))
    }

    /// Require the restart path to currently exist and be a file. This is the default.
    pub fn require_existing_file(mut self) -> Self {
        self.require_existing_file = true;
        self
    }

    /// Allow missing restart paths, matching the lower-level platform API.
    pub fn allow_missing(mut self) -> Self {
        self.require_existing_file = false;
        self
    }

    /// Canonicalize the restart path before storing it.
    pub fn canonicalize(mut self) -> Self {
        self.canonicalize_path = true;
        self
    }

    /// Preserve the restart path exactly as configured. This is the default.
    pub fn preserve_path(mut self) -> Self {
        self.canonicalize_path = false;
        self
    }

    /// Return the configured restart path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the restart path must exist as a file.
    pub fn requires_existing_file(&self) -> bool {
        self.require_existing_file
    }

    /// Whether the restart path will be canonicalized before storage.
    pub fn canonicalizes_path(&self) -> bool {
        self.canonicalize_path
    }

    /// Validate the restart path.
    pub fn validate(&self) -> Result<()> {
        self.resolve_path()?;
        Ok(())
    }

    /// Build the validated restart path.
    pub fn build_checked(self) -> Result<PathBuf> {
        self.resolve_path()
    }

    fn resolve_path(&self) -> Result<PathBuf> {
        anyhow::ensure!(
            !self.path.as_os_str().is_empty(),
            "restart path cannot be empty"
        );

        let path = if self.canonicalize_path {
            fs::canonicalize(&self.path).with_context(|| {
                format!(
                    "failed to canonicalize restart path {}",
                    self.path.display()
                )
            })?
        } else {
            self.path.clone()
        };

        if self.require_existing_file {
            anyhow::ensure!(
                path.is_file(),
                "restart path must be an existing file: {}",
                path.display()
            );
        }

        Ok(path)
    }
}

/// Builder for a dock/taskbar badge label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockBadgeBuilder {
    label: Option<String>,
}

impl DockBadgeBuilder {
    /// Clear the dock/taskbar badge.
    pub fn clear() -> Self {
        Self { label: None }
    }

    /// Set a badge label such as `"syncing"`, `"!"`, or `"3+"`.
    pub fn label(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
        }
    }

    /// Set a numeric badge count.
    pub fn count(count: u32) -> Self {
        Self::label(count.to_string())
    }

    /// Return the configured badge label, or `None` when the badge should be cleared.
    pub fn label_text(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Return whether this builder clears the badge.
    pub fn is_clear(&self) -> bool {
        self.label.is_none()
    }

    /// Validate the badge label before passing it to platform rendering.
    pub fn validate(&self) -> Result<()> {
        let Some(label) = &self.label else {
            return Ok(());
        };

        anyhow::ensure!(!label.trim().is_empty(), "dock badge label cannot be empty");
        anyhow::ensure!(
            label == label.trim(),
            "dock badge label cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            !label.chars().any(char::is_control),
            "dock badge label cannot contain control characters"
        );
        anyhow::ensure!(
            label.chars().count() <= 16,
            "dock badge label cannot be longer than 16 characters"
        );
        Ok(())
    }

    /// Build the validated badge label.
    pub fn build_checked(self) -> Result<Option<String>> {
        self.validate()?;
        Ok(self.label)
    }
}

impl Default for DockBadgeBuilder {
    fn default() -> Self {
        Self::clear()
    }
}

/// Builder for a system tray tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayTooltipBuilder {
    tooltip: Option<String>,
}

impl TrayTooltipBuilder {
    /// Clear the system tray tooltip.
    pub fn clear() -> Self {
        Self { tooltip: None }
    }

    /// Set a user-facing tray tooltip such as `"Sync complete"`.
    pub fn text(tooltip: impl Into<String>) -> Self {
        Self {
            tooltip: Some(tooltip.into()),
        }
    }

    /// Set a tooltip from a short status label.
    pub fn status(status: impl Into<String>) -> Self {
        Self::text(status)
    }

    /// Return the configured tooltip, or `None` when the tooltip should be cleared.
    pub fn tooltip_text(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    /// Return whether this builder clears the tooltip.
    pub fn is_clear(&self) -> bool {
        self.tooltip.is_none()
    }

    /// Validate tooltip text before passing it to platform UI.
    pub fn validate(&self) -> Result<()> {
        let Some(tooltip) = &self.tooltip else {
            return Ok(());
        };

        anyhow::ensure!(!tooltip.trim().is_empty(), "tray tooltip cannot be empty");
        anyhow::ensure!(
            tooltip == tooltip.trim(),
            "tray tooltip cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            !tooltip.chars().any(char::is_control),
            "tray tooltip cannot contain control characters"
        );
        anyhow::ensure!(
            tooltip.chars().count() <= 256,
            "tray tooltip cannot be longer than 256 characters"
        );
        Ok(())
    }

    /// Build the validated tooltip text.
    pub fn build_checked(self) -> Result<Option<String>> {
        self.validate()?;
        Ok(self.tooltip)
    }
}

impl Default for TrayTooltipBuilder {
    fn default() -> Self {
        Self::clear()
    }
}

/// A validated system-tray/background-app configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayAppConfig {
    menu: Vec<TrayMenuItem>,
    tooltip: Option<String>,
    panel_mode: bool,
    keep_alive_without_windows: bool,
}

impl TrayAppConfig {
    /// The validated tray menu items.
    pub fn menu(&self) -> &[TrayMenuItem] {
        &self.menu
    }

    /// The validated tooltip text, or `None` when the tooltip should be cleared.
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    /// Whether tray icon clicks should route as panel events.
    pub fn panel_mode(&self) -> bool {
        self.panel_mode
    }

    /// Whether the app should stay alive when all windows close.
    pub fn keep_alive_without_windows(&self) -> bool {
        self.keep_alive_without_windows
    }
}

/// Builder for installing tray menu, tooltip, panel behavior, and background lifetime together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayAppBuilder {
    menu: TrayMenuBuilder,
    tooltip: TrayTooltipBuilder,
    panel_mode: bool,
    keep_alive_without_windows: bool,
}

impl TrayAppBuilder {
    /// Create a tray-app builder with an empty menu and cleared tooltip.
    pub fn new() -> Self {
        Self {
            menu: TrayMenuBuilder::new(),
            tooltip: TrayTooltipBuilder::clear(),
            panel_mode: false,
            keep_alive_without_windows: true,
        }
    }

    /// Set the tray menu builder.
    pub fn menu(mut self, menu: TrayMenuBuilder) -> Self {
        self.menu = menu;
        self
    }

    /// Add a clickable tray menu action.
    pub fn action(mut self, label: impl Into<SharedString>, id: impl Into<SharedString>) -> Self {
        self.menu = self.menu.action(label, id);
        self
    }

    /// Add a separator to the tray menu.
    pub fn separator(mut self) -> Self {
        self.menu = self.menu.separator();
        self
    }

    /// Add a toggleable tray menu item.
    pub fn toggle(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        id: impl Into<SharedString>,
    ) -> Self {
        self.menu = self.menu.toggle(label, checked, id);
        self
    }

    /// Add a submenu to the tray menu.
    pub fn submenu(
        mut self,
        label: impl Into<SharedString>,
        items: impl Into<Vec<TrayMenuItem>>,
    ) -> Self {
        self.menu = self.menu.submenu(label, items);
        self
    }

    /// Set the tray tooltip builder.
    pub fn tooltip(mut self, tooltip: TrayTooltipBuilder) -> Self {
        self.tooltip = tooltip;
        self
    }

    /// Set a short tray status tooltip.
    pub fn status_tooltip(self, tooltip: impl Into<String>) -> Self {
        self.tooltip(TrayTooltipBuilder::status(tooltip))
    }

    /// Clear the tray tooltip.
    pub fn clear_tooltip(self) -> Self {
        self.tooltip(TrayTooltipBuilder::clear())
    }

    /// Enable or disable tray panel mode.
    pub fn panel_mode(mut self, enabled: bool) -> Self {
        self.panel_mode = enabled;
        self
    }

    /// Enable panel mode for tray click handling.
    pub fn panel(mut self) -> Self {
        self.panel_mode = true;
        self
    }

    /// Set whether the application stays alive after closing all windows.
    pub fn keep_alive_without_windows(mut self, keep_alive: bool) -> Self {
        self.keep_alive_without_windows = keep_alive;
        self
    }

    /// Return the configured tray menu builder.
    pub fn menu_builder(&self) -> &TrayMenuBuilder {
        &self.menu
    }

    /// Return the configured tooltip builder.
    pub fn tooltip_builder(&self) -> &TrayTooltipBuilder {
        &self.tooltip
    }

    /// Validate the tray-app configuration.
    pub fn validate(&self) -> Result<()> {
        self.menu.validate()?;
        self.tooltip.validate()?;
        Ok(())
    }

    /// Build the validated tray-app configuration.
    pub fn build_checked(self) -> Result<TrayAppConfig> {
        self.validate()?;
        Ok(TrayAppConfig {
            menu: self.menu.build()?,
            tooltip: self.tooltip.build_checked()?,
            panel_mode: self.panel_mode,
            keep_alive_without_windows: self.keep_alive_without_windows,
        })
    }
}

impl Default for TrayAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// How the application should behave when all windows are closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCloseBehavior {
    /// Quit when the final window closes.
    QuitWhenAllWindowsClose,
    /// Keep the process alive after all windows close.
    KeepAliveWithoutWindows,
}

impl WindowCloseBehavior {
    /// Return whether the platform should keep the app alive without windows.
    pub fn keep_alive_without_windows(self) -> bool {
        matches!(self, Self::KeepAliveWithoutWindows)
    }
}

/// Validated app lifecycle configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLifecyclePolicy {
    window_close_behavior: WindowCloseBehavior,
    quit_cleanup_timeout: Duration,
    reason: Option<String>,
}

impl AppLifecyclePolicy {
    /// Behavior used when the final window closes.
    pub fn window_close_behavior(&self) -> WindowCloseBehavior {
        self.window_close_behavior
    }

    /// Whether the app stays alive after all windows close.
    pub fn keep_alive_without_windows(&self) -> bool {
        self.window_close_behavior.keep_alive_without_windows()
    }

    /// Timeout for futures registered with `on_app_quit`.
    pub fn quit_cleanup_timeout(&self) -> Duration {
        self.quit_cleanup_timeout
    }

    /// Optional human-readable reason for telemetry or startup diagnostics.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// Builder for configuring app lifetime and bounded quit cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLifecyclePolicyBuilder {
    window_close_behavior: WindowCloseBehavior,
    quit_cleanup_timeout: Duration,
    reason: Option<String>,
}

impl AppLifecyclePolicyBuilder {
    /// Create the default lifecycle policy.
    pub fn new() -> Self {
        Self {
            window_close_behavior: WindowCloseBehavior::QuitWhenAllWindowsClose,
            quit_cleanup_timeout: SHUTDOWN_TIMEOUT,
            reason: None,
        }
    }

    /// Quit when the final app window closes.
    pub fn quit_when_all_windows_close(mut self) -> Self {
        self.window_close_behavior = WindowCloseBehavior::QuitWhenAllWindowsClose;
        self
    }

    /// Keep the app process alive when all windows close.
    pub fn keep_alive_without_windows(mut self) -> Self {
        self.window_close_behavior = WindowCloseBehavior::KeepAliveWithoutWindows;
        self
    }

    /// Set the timeout for `on_app_quit` cleanup futures.
    pub fn quit_cleanup_timeout(mut self, timeout: Duration) -> Self {
        self.quit_cleanup_timeout = timeout;
        self
    }

    /// Set a human-readable reason for lifecycle diagnostics.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Return the configured final-window behavior.
    pub fn window_close_behavior(&self) -> WindowCloseBehavior {
        self.window_close_behavior
    }

    /// Return the configured quit cleanup timeout.
    pub fn configured_quit_cleanup_timeout(&self) -> Duration {
        self.quit_cleanup_timeout
    }

    /// Validate lifecycle policy before applying platform state.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.quit_cleanup_timeout > Duration::ZERO,
            "quit cleanup timeout must be greater than zero"
        );
        anyhow::ensure!(
            self.quit_cleanup_timeout <= Duration::from_secs(30),
            "quit cleanup timeout cannot exceed 30 seconds"
        );
        if let Some(reason) = &self.reason {
            validate_app_metadata_text(reason, "lifecycle policy reason", 256, true)?;
        }
        Ok(())
    }

    /// Build the validated lifecycle policy.
    pub fn build_checked(self) -> Result<AppLifecyclePolicy> {
        self.validate()?;
        Ok(AppLifecyclePolicy {
            window_close_behavior: self.window_close_behavior,
            quit_cleanup_timeout: self.quit_cleanup_timeout,
            reason: self.reason,
        })
    }
}

impl Default for AppLifecyclePolicyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// App-level lifecycle or activation command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLifecycleCommandKind {
    /// Bring the app to the foreground.
    Activate {
        /// Whether to ask the OS to ignore other apps while activating.
        ignoring_other_apps: bool,
    },
    /// Hide this app.
    Hide,
    /// Hide other apps.
    HideOtherApps,
    /// Unhide other apps.
    UnhideOtherApps,
    /// Quit the app.
    Quit,
    /// Restart/relaunch the app.
    Restart,
}

/// Checked app-level lifecycle command for Electron-style app control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLifecycleCommand {
    kind: AppLifecycleCommandKind,
    reason: Option<String>,
}

impl AppLifecycleCommand {
    /// Bring the app to the foreground without forcing other apps aside.
    pub fn activate() -> Self {
        Self::activate_with_options(false)
    }

    /// Bring the app to the foreground, optionally asking the OS to ignore other apps.
    pub fn activate_with_options(ignoring_other_apps: bool) -> Self {
        Self {
            kind: AppLifecycleCommandKind::Activate {
                ignoring_other_apps,
            },
            reason: None,
        }
    }

    /// Hide this app.
    pub fn hide() -> Self {
        Self {
            kind: AppLifecycleCommandKind::Hide,
            reason: None,
        }
    }

    /// Hide other apps.
    pub fn hide_other_apps() -> Self {
        Self {
            kind: AppLifecycleCommandKind::HideOtherApps,
            reason: None,
        }
    }

    /// Unhide other apps.
    pub fn unhide_other_apps() -> Self {
        Self {
            kind: AppLifecycleCommandKind::UnhideOtherApps,
            reason: None,
        }
    }

    /// Quit the app with a required human-readable reason.
    pub fn quit(reason: impl Into<String>) -> Self {
        Self {
            kind: AppLifecycleCommandKind::Quit,
            reason: Some(reason.into()),
        }
    }

    /// Restart the app with a required human-readable reason.
    pub fn restart(reason: impl Into<String>) -> Self {
        Self {
            kind: AppLifecycleCommandKind::Restart,
            reason: Some(reason.into()),
        }
    }

    /// Attach a diagnostic reason to a non-terminal activation/hide command.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The command kind.
    pub fn kind(&self) -> AppLifecycleCommandKind {
        self.kind
    }

    /// Optional human-readable reason for telemetry or command auditing.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Whether this command can terminate the current process.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.kind,
            AppLifecycleCommandKind::Quit | AppLifecycleCommandKind::Restart
        )
    }

    /// Validate the command before dispatching it to the platform.
    pub fn validate(&self) -> Result<()> {
        if self.is_terminal() {
            anyhow::ensure!(
                self.reason.is_some(),
                "quit and restart lifecycle commands require a reason"
            );
        }

        if let Some(reason) = &self.reason {
            validate_app_metadata_text(reason, "lifecycle command reason", 256, true)?;
        }

        Ok(())
    }
}

/// Target for an app-owned visual capture request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppWindowCaptureTarget {
    /// Capture the currently focused app window.
    FocusedWindow,
    /// Capture a specific app window.
    Window(WindowId),
    /// Capture every visible app window as separate frames.
    VisibleAppWindows,
}

/// Output encoding requested for an app-owned visual capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppWindowCaptureFormat {
    /// PNG-encoded image bytes.
    Png,
    /// Raw RGBA pixels for tests or custom encoders.
    Rgba,
}

/// Checked app-window visual capture request for tests, diagnostics, and agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppWindowCaptureRequest {
    purpose: String,
    target: AppWindowCaptureTarget,
    format: AppWindowCaptureFormat,
    include_window_chrome: bool,
    include_cursor: bool,
    allow_occluded: bool,
    max_width_px: Option<u32>,
    max_height_px: Option<u32>,
    max_pixels: Option<u64>,
}

impl AppWindowCaptureRequest {
    /// Start a focused-window capture request.
    pub fn focused_window(purpose: impl Into<String>) -> AppWindowCaptureRequestBuilder {
        AppWindowCaptureRequestBuilder::focused_window(purpose)
    }

    /// Start a specific-window capture request.
    pub fn window(
        window_id: WindowId,
        purpose: impl Into<String>,
    ) -> AppWindowCaptureRequestBuilder {
        AppWindowCaptureRequestBuilder::window(window_id, purpose)
    }

    /// Start an all-visible-app-windows capture request.
    pub fn visible_app_windows(purpose: impl Into<String>) -> AppWindowCaptureRequestBuilder {
        AppWindowCaptureRequestBuilder::visible_app_windows(purpose)
    }

    /// User-facing reason for capture.
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Capture target.
    pub fn target(&self) -> AppWindowCaptureTarget {
        self.target
    }

    /// Requested output format.
    pub fn format(&self) -> AppWindowCaptureFormat {
        self.format
    }

    /// Whether native window chrome should be included when the backend supports it.
    pub fn includes_window_chrome(&self) -> bool {
        self.include_window_chrome
    }

    /// Whether the pointer/cursor should be included when the backend supports it.
    pub fn includes_cursor(&self) -> bool {
        self.include_cursor
    }

    /// Whether capture may use an OS-level snapshot when the window is occluded.
    pub fn allows_occluded(&self) -> bool {
        self.allow_occluded
    }

    /// Optional maximum output width.
    pub fn max_width_px(&self) -> Option<u32> {
        self.max_width_px
    }

    /// Optional maximum output height.
    pub fn max_height_px(&self) -> Option<u32> {
        self.max_height_px
    }

    /// Optional maximum total output pixels.
    pub fn max_pixels(&self) -> Option<u64> {
        self.max_pixels
    }

    /// Capability required when the request may use OS-level occluded/window capture.
    pub fn required_capability(&self) -> Option<Capability> {
        self.allow_occluded.then_some(Capability::ScreenCapture)
    }

    /// Validate request shape before dispatching to a capture backend.
    pub fn validate(&self) -> Result<()> {
        validate_app_metadata_text(&self.purpose, "app window capture purpose", 160, false)?;
        if let Some(max_width_px) = self.max_width_px {
            validate_capture_dimension(max_width_px, "app window capture max width")?;
        }
        if let Some(max_height_px) = self.max_height_px {
            validate_capture_dimension(max_height_px, "app window capture max height")?;
        }
        if let Some(max_pixels) = self.max_pixels {
            anyhow::ensure!(
                max_pixels > 0,
                "app window capture max pixels must be greater than zero"
            );
            anyhow::ensure!(
                max_pixels <= 268_435_456,
                "app window capture max pixels cannot exceed 268435456"
            );
        }
        anyhow::ensure!(
            !(matches!(self.target, AppWindowCaptureTarget::VisibleAppWindows)
                && self.include_cursor),
            "app window capture cannot include cursor when capturing multiple windows"
        );
        Ok(())
    }
}

/// Builder for checked app-window visual capture requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppWindowCaptureRequestBuilder {
    purpose: String,
    target: AppWindowCaptureTarget,
    format: AppWindowCaptureFormat,
    include_window_chrome: bool,
    include_cursor: bool,
    allow_occluded: bool,
    max_width_px: Option<u32>,
    max_height_px: Option<u32>,
    max_pixels: Option<u64>,
}

impl AppWindowCaptureRequestBuilder {
    /// Create a request for the given target.
    pub fn new(target: AppWindowCaptureTarget, purpose: impl Into<String>) -> Self {
        Self {
            purpose: purpose.into(),
            target,
            format: AppWindowCaptureFormat::Png,
            include_window_chrome: false,
            include_cursor: false,
            allow_occluded: false,
            max_width_px: Some(4096),
            max_height_px: Some(4096),
            max_pixels: Some(16_777_216),
        }
    }

    /// Capture the currently focused app window.
    pub fn focused_window(purpose: impl Into<String>) -> Self {
        Self::new(AppWindowCaptureTarget::FocusedWindow, purpose)
    }

    /// Capture a specific app window.
    pub fn window(window_id: WindowId, purpose: impl Into<String>) -> Self {
        Self::new(AppWindowCaptureTarget::Window(window_id), purpose)
    }

    /// Capture every visible app window as separate frames.
    pub fn visible_app_windows(purpose: impl Into<String>) -> Self {
        Self::new(AppWindowCaptureTarget::VisibleAppWindows, purpose)
    }

    /// Request PNG output.
    pub fn png(mut self) -> Self {
        self.format = AppWindowCaptureFormat::Png;
        self
    }

    /// Request raw RGBA output.
    pub fn rgba(mut self) -> Self {
        self.format = AppWindowCaptureFormat::Rgba;
        self
    }

    /// Include native window chrome when the backend supports it.
    pub fn include_window_chrome(mut self) -> Self {
        self.include_window_chrome = true;
        self
    }

    /// Include the pointer/cursor when the backend supports it.
    pub fn include_cursor(mut self) -> Self {
        self.include_cursor = true;
        self
    }

    /// Permit OS-level capture for occluded/minimized windows.
    pub fn allow_occluded(mut self) -> Self {
        self.allow_occluded = true;
        self
    }

    /// Set maximum output dimensions.
    pub fn max_dimensions(mut self, width_px: u32, height_px: u32) -> Self {
        self.max_width_px = Some(width_px);
        self.max_height_px = Some(height_px);
        self
    }

    /// Remove maximum output dimension checks.
    pub fn unlimited_dimensions(mut self) -> Self {
        self.max_width_px = None;
        self.max_height_px = None;
        self
    }

    /// Set maximum total output pixels.
    pub fn max_pixels(mut self, max_pixels: u64) -> Self {
        self.max_pixels = Some(max_pixels);
        self
    }

    /// Remove total pixel checks.
    pub fn unlimited_pixels(mut self) -> Self {
        self.max_pixels = None;
        self
    }

    /// Validate this capture request.
    pub fn validate(&self) -> Result<()> {
        self.as_request().validate()
    }

    /// Build a checked capture request.
    pub fn build_checked(self) -> Result<AppWindowCaptureRequest> {
        let request = self.as_request();
        request.validate()?;
        Ok(request)
    }

    fn as_request(&self) -> AppWindowCaptureRequest {
        AppWindowCaptureRequest {
            purpose: self.purpose.clone(),
            target: self.target,
            format: self.format,
            include_window_chrome: self.include_window_chrome,
            include_cursor: self.include_cursor,
            allow_occluded: self.allow_occluded,
            max_width_px: self.max_width_px,
            max_height_px: self.max_height_px,
            max_pixels: self.max_pixels,
        }
    }
}

/// Read-only snapshot of app runtime state for startup gates and agent audits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRuntimeSnapshot {
    process_id: ProcessId,
    uptime: Duration,
    window_count: usize,
    keep_alive_without_windows: bool,
    quit_cleanup_timeout: Duration,
    quitting: bool,
    network_status: NetworkStatus,
    power: SystemPowerSnapshot,
    theme: NativeThemeSnapshot,
}

impl AppRuntimeSnapshot {
    /// Process identifier used for capability checks.
    pub fn process_id(&self) -> ProcessId {
        self.process_id
    }

    /// Duration since the Kael app state was created.
    pub fn uptime(&self) -> Duration {
        self.uptime
    }

    /// Number of open Kael windows.
    pub fn window_count(&self) -> usize {
        self.window_count
    }

    /// Whether the app is configured to stay alive without windows.
    pub fn keep_alive_without_windows(&self) -> bool {
        self.keep_alive_without_windows
    }

    /// Timeout for futures registered with `on_app_quit`.
    pub fn quit_cleanup_timeout(&self) -> Duration {
        self.quit_cleanup_timeout
    }

    /// Whether app shutdown has begun.
    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// Current network connectivity status.
    pub fn network_status(&self) -> NetworkStatus {
        self.network_status
    }

    /// Current power, idle, and reduce-motion state.
    pub fn power(&self) -> &SystemPowerSnapshot {
        &self.power
    }

    /// Current native theme and accessibility state.
    pub fn theme(&self) -> NativeThemeSnapshot {
        self.theme
    }

    /// Whether the snapshot represents a background/tray-style runtime.
    pub fn is_background_runtime(&self) -> bool {
        self.keep_alive_without_windows && self.window_count == 0
    }
}

/// Builder for a biometric authentication prompt.
#[derive(Debug, Clone)]
pub struct BiometricAuthBuilder {
    reason: String,
    require_available: bool,
}

impl BiometricAuthBuilder {
    /// Create a biometric prompt with the user-facing reason shown by the OS.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            require_available: true,
        }
    }

    /// Create a common vault-unlock biometric prompt.
    pub fn unlock_vault() -> Self {
        Self::new("Unlock your vault")
    }

    /// Create a common payment-approval biometric prompt.
    pub fn approve_payment() -> Self {
        Self::new("Approve payment")
    }

    /// Allow forwarding the prompt to the platform even when Kael cannot detect
    /// biometric availability up front.
    pub fn allow_unavailable(mut self) -> Self {
        self.require_available = false;
        self
    }

    /// Return the configured prompt reason.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Return whether this prompt requires an available biometric method.
    pub fn requires_available(&self) -> bool {
        self.require_available
    }

    /// Validate the prompt copy before asking the platform to show UI.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.reason.trim().is_empty(),
            "biometric authentication reason cannot be empty"
        );
        anyhow::ensure!(
            self.reason == self.reason.trim(),
            "biometric authentication reason cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            self.reason.len() <= 256,
            "biometric authentication reason cannot be longer than 256 bytes"
        );
        anyhow::ensure!(
            !self.reason.chars().any(char::is_control),
            "biometric authentication reason cannot contain control characters"
        );
        Ok(())
    }

    /// Build the validated prompt reason and availability policy.
    pub fn build_checked(self) -> Result<(String, bool)> {
        self.validate()?;
        Ok((self.reason, self.require_available))
    }
}

/// Snapshot returned when requesting a biometric prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiometricAuthRequest {
    status: BiometricStatus,
    prompted: bool,
    reason: String,
}

impl BiometricAuthRequest {
    /// The biometric availability observed before the request.
    pub fn status(&self) -> BiometricStatus {
        self.status
    }

    /// Whether Kael forwarded the prompt to the platform.
    pub fn prompted(&self) -> bool {
        self.prompted
    }

    /// The validated reason used for the prompt.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

type NetworkStatusCallback = Box<dyn FnMut(NetworkStatus, &mut App)>;
type NetworkTransitionCallback = Box<dyn FnMut(&mut App)>;

/// Snapshot of system state relevant to adaptive rendering and background work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPowerSnapshot {
    power_mode: PowerMode,
    reduce_motion: bool,
    idle_time: Option<Duration>,
}

impl SystemPowerSnapshot {
    /// Current OS power policy.
    pub fn power_mode(&self) -> PowerMode {
        self.power_mode
    }

    /// Whether the OS reduce-motion accessibility preference is enabled.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Duration since the last user input event, when the platform exposes it.
    pub fn idle_time(&self) -> Option<Duration> {
        self.idle_time
    }

    /// Whether apps should prefer lower CPU/GPU work for this snapshot.
    pub fn should_reduce_work(&self) -> bool {
        self.reduce_motion || matches!(self.power_mode, PowerMode::LowPower)
    }

    /// Evaluate this snapshot against an idle policy.
    pub fn evaluate_idle(&self, policy: &SystemIdlePolicy) -> SystemIdleEvaluation {
        policy.evaluate(self)
    }
}

/// Result of evaluating system idle time against a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemIdleEvaluation {
    /// The platform reported enough idle time to satisfy the threshold.
    Idle {
        /// Reported idle duration.
        idle_time: Duration,
        /// Required idle threshold.
        threshold: Duration,
    },
    /// The platform reported user activity newer than the threshold.
    Active {
        /// Reported idle duration.
        idle_time: Duration,
        /// Required idle threshold.
        threshold: Duration,
    },
    /// The platform does not expose idle time for this snapshot.
    Unknown {
        /// Required idle threshold.
        threshold: Duration,
        /// Whether the policy treats unknown idle time as allowed.
        treated_as_idle: bool,
    },
}

impl SystemIdleEvaluation {
    /// Return true when this evaluation allows idle-gated work to run.
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle { .. })
            || matches!(
                self,
                Self::Unknown {
                    treated_as_idle: true,
                    ..
                }
            )
    }

    /// Return the reported idle duration, if known.
    pub fn idle_time(&self) -> Option<Duration> {
        match self {
            Self::Idle { idle_time, .. } | Self::Active { idle_time, .. } => Some(*idle_time),
            Self::Unknown { .. } => None,
        }
    }

    /// Return the policy threshold used for this evaluation.
    pub fn threshold(&self) -> Duration {
        match self {
            Self::Idle { threshold, .. }
            | Self::Active { threshold, .. }
            | Self::Unknown { threshold, .. } => *threshold,
        }
    }
}

/// A checked policy for deciding when the system has been idle long enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemIdlePolicy {
    threshold: Duration,
    require_known_idle_time: bool,
    treat_unknown_as_idle: bool,
}

impl SystemIdlePolicy {
    /// Create an idle policy builder with the required idle threshold.
    pub fn builder(threshold: Duration) -> SystemIdlePolicyBuilder {
        SystemIdlePolicyBuilder::new(threshold)
    }

    /// Return the idle threshold.
    pub fn threshold(&self) -> Duration {
        self.threshold
    }

    /// Return whether idle time must be known for the policy to match.
    pub fn requires_known_idle_time(&self) -> bool {
        self.require_known_idle_time
    }

    /// Return whether missing idle data should be treated as idle.
    pub fn treats_unknown_as_idle(&self) -> bool {
        self.treat_unknown_as_idle
    }

    /// Validate the idle policy.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.threshold > Duration::ZERO,
            "system idle threshold must be greater than zero"
        );
        anyhow::ensure!(
            !(self.require_known_idle_time && self.treat_unknown_as_idle),
            "system idle policy cannot both require known idle time and treat unknown as idle"
        );
        Ok(())
    }

    /// Evaluate a system power snapshot against this policy.
    pub fn evaluate(&self, snapshot: &SystemPowerSnapshot) -> SystemIdleEvaluation {
        match snapshot.idle_time() {
            Some(idle_time) if idle_time >= self.threshold => SystemIdleEvaluation::Idle {
                idle_time,
                threshold: self.threshold,
            },
            Some(idle_time) => SystemIdleEvaluation::Active {
                idle_time,
                threshold: self.threshold,
            },
            None => SystemIdleEvaluation::Unknown {
                threshold: self.threshold,
                treated_as_idle: self.treat_unknown_as_idle && !self.require_known_idle_time,
            },
        }
    }

    /// Return true when idle-gated work should run for this snapshot.
    pub fn allows(&self, snapshot: &SystemPowerSnapshot) -> bool {
        self.evaluate(snapshot).is_idle()
    }
}

/// Builder for checked system-idle policies.
#[derive(Debug, Clone, Copy)]
pub struct SystemIdlePolicyBuilder {
    policy: SystemIdlePolicy,
}

impl SystemIdlePolicyBuilder {
    /// Create a builder with the required idle threshold.
    pub fn new(threshold: Duration) -> Self {
        Self {
            policy: SystemIdlePolicy {
                threshold,
                require_known_idle_time: false,
                treat_unknown_as_idle: false,
            },
        }
    }

    /// Create a builder from whole seconds.
    pub fn seconds(seconds: u64) -> Self {
        Self::new(Duration::from_secs(seconds))
    }

    /// Create a builder from whole minutes.
    pub fn minutes(minutes: u64) -> Self {
        Self::new(Duration::from_secs(minutes.saturating_mul(60)))
    }

    /// Require platforms to report idle time before the policy can match.
    pub fn require_known_idle_time(mut self) -> Self {
        self.policy.require_known_idle_time = true;
        self
    }

    /// Allow idle-gated work when the platform cannot report idle time.
    pub fn treat_unknown_as_idle(mut self) -> Self {
        self.policy.treat_unknown_as_idle = true;
        self
    }

    /// Validate the configured policy.
    pub fn validate(&self) -> Result<()> {
        self.policy.validate()
    }

    /// Build the checked idle policy.
    pub fn build_checked(self) -> Result<SystemIdlePolicy> {
        self.policy.validate()?;
        Ok(self.policy)
    }
}

impl From<SystemIdlePolicy> for SystemIdlePolicyBuilder {
    fn from(policy: SystemIdlePolicy) -> Self {
        Self { policy }
    }
}

type SystemPowerEventCallback = Box<dyn FnMut(SystemPowerEvent, &SystemPowerSnapshot, &mut App)>;
type SystemPowerTransitionCallback = Box<dyn FnMut(&SystemPowerSnapshot, &mut App)>;

/// Builder for monitoring system power, idle, and motion-preference changes.
#[derive(Default)]
pub struct SystemPowerMonitorBuilder {
    on_event: Option<SystemPowerEventCallback>,
    on_suspend: Option<SystemPowerTransitionCallback>,
    on_resume: Option<SystemPowerTransitionCallback>,
    on_power_mode_changed: Option<SystemPowerTransitionCallback>,
    on_lock_screen: Option<SystemPowerTransitionCallback>,
    on_unlock_screen: Option<SystemPowerTransitionCallback>,
    on_shutdown: Option<SystemPowerTransitionCallback>,
}

impl SystemPowerMonitorBuilder {
    /// Create an empty system-power monitor builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle every system power event with the latest adaptive snapshot.
    pub fn on_event(
        mut self,
        callback: impl FnMut(SystemPowerEvent, &SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_event = Some(Box::new(callback));
        self
    }

    /// Handle system suspend/sleep.
    pub fn on_suspend(
        mut self,
        callback: impl FnMut(&SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_suspend = Some(Box::new(callback));
        self
    }

    /// Handle system resume/wake.
    pub fn on_resume(
        mut self,
        callback: impl FnMut(&SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_resume = Some(Box::new(callback));
        self
    }

    /// Handle power-policy changes, such as entering battery saver.
    pub fn on_power_mode_changed(
        mut self,
        callback: impl FnMut(&SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_power_mode_changed = Some(Box::new(callback));
        self
    }

    /// Handle screen lock.
    pub fn on_lock_screen(
        mut self,
        callback: impl FnMut(&SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_lock_screen = Some(Box::new(callback));
        self
    }

    /// Handle screen unlock.
    pub fn on_unlock_screen(
        mut self,
        callback: impl FnMut(&SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_unlock_screen = Some(Box::new(callback));
        self
    }

    /// Handle system shutdown.
    pub fn on_shutdown(
        mut self,
        callback: impl FnMut(&SystemPowerSnapshot, &mut App) + 'static,
    ) -> Self {
        self.on_shutdown = Some(Box::new(callback));
        self
    }

    /// Return true when any callback has been configured.
    pub fn has_callbacks(&self) -> bool {
        self.on_event.is_some()
            || self.on_suspend.is_some()
            || self.on_resume.is_some()
            || self.on_power_mode_changed.is_some()
            || self.on_lock_screen.is_some()
            || self.on_unlock_screen.is_some()
            || self.on_shutdown.is_some()
    }

    /// Validate that the monitor has at least one callback.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.has_callbacks(),
            "system power monitor must configure at least one callback"
        );
        Ok(())
    }

    fn into_parts(
        self,
    ) -> (
        Option<SystemPowerEventCallback>,
        Option<SystemPowerTransitionCallback>,
        Option<SystemPowerTransitionCallback>,
        Option<SystemPowerTransitionCallback>,
        Option<SystemPowerTransitionCallback>,
        Option<SystemPowerTransitionCallback>,
        Option<SystemPowerTransitionCallback>,
    ) {
        (
            self.on_event,
            self.on_suspend,
            self.on_resume,
            self.on_power_mode_changed,
            self.on_lock_screen,
            self.on_unlock_screen,
            self.on_shutdown,
        )
    }
}

/// Active system-power monitor returned by [`App::watch_system_power`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPowerMonitor {
    initial_snapshot: SystemPowerSnapshot,
}

impl SystemPowerMonitor {
    /// Initial adaptive snapshot captured before subscribing to power events.
    pub fn initial_snapshot(&self) -> &SystemPowerSnapshot {
        &self.initial_snapshot
    }

    /// Whether the initial snapshot suggests reducing CPU/GPU work.
    pub fn initially_should_reduce_work(&self) -> bool {
        self.initial_snapshot.should_reduce_work()
    }
}

/// Snapshot of native theme and accessibility signals for app UI decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeThemeSnapshot {
    appearance: WindowAppearance,
    reduce_motion: bool,
    power_mode: PowerMode,
}

impl NativeThemeSnapshot {
    /// Create a snapshot from known platform signals.
    pub fn new(appearance: WindowAppearance, reduce_motion: bool, power_mode: PowerMode) -> Self {
        Self {
            appearance,
            reduce_motion,
            power_mode,
        }
    }

    /// Current OS window appearance.
    pub fn appearance(&self) -> WindowAppearance {
        self.appearance
    }

    /// Whether the current appearance is dark.
    pub fn is_dark(&self) -> bool {
        self.appearance.is_dark()
    }

    /// Whether the current appearance is light.
    pub fn is_light(&self) -> bool {
        self.appearance.is_light()
    }

    /// Whether the current appearance uses platform vibrancy/material effects.
    pub fn is_vibrant(&self) -> bool {
        self.appearance.is_vibrant()
    }

    /// Whether the OS reduce-motion accessibility preference is enabled.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Current OS power policy.
    pub fn power_mode(&self) -> PowerMode {
        self.power_mode
    }

    /// Whether UI should reduce non-essential motion, blur, effects, or polling.
    pub fn should_reduce_effects(&self) -> bool {
        self.reduce_motion || matches!(self.power_mode, PowerMode::LowPower)
    }

    /// Choose between dark and light values based on the native appearance.
    pub fn choose<T>(&self, dark: T, light: T) -> T {
        if self.is_dark() { dark } else { light }
    }
}

/// Builder for resolving a semantic desktop window placement into screen bounds.
#[derive(Debug, Clone)]
pub struct WindowPlacementBuilder {
    size: Size<Pixels>,
    position: WindowPosition,
}

impl WindowPlacementBuilder {
    /// Create a placement centered on the primary display.
    pub fn new(size: Size<Pixels>) -> Self {
        Self {
            size,
            position: WindowPosition::Center,
        }
    }

    /// Center on the primary display.
    pub fn center(mut self) -> Self {
        self.position = WindowPosition::Center;
        self
    }

    /// Center on a specific display.
    pub fn center_on_display(mut self, display_id: DisplayId) -> Self {
        self.position = WindowPosition::CenterOnDisplay(display_id);
        self
    }

    /// Center above a tray icon's bounds.
    pub fn tray_center(mut self, tray_bounds: Bounds<Pixels>) -> Self {
        self.position = WindowPosition::TrayCenter(tray_bounds);
        self
    }

    /// Place in the top-right corner of the primary display.
    pub fn top_right(mut self, margin: Pixels) -> Self {
        self.position = WindowPosition::TopRight { margin };
        self
    }

    /// Place in the bottom-right corner of the primary display.
    pub fn bottom_right(mut self, margin: Pixels) -> Self {
        self.position = WindowPosition::BottomRight { margin };
        self
    }

    /// Place in the top-left corner of the primary display.
    pub fn top_left(mut self, margin: Pixels) -> Self {
        self.position = WindowPosition::TopLeft { margin };
        self
    }

    /// Place in the bottom-left corner of the primary display.
    pub fn bottom_left(mut self, margin: Pixels) -> Self {
        self.position = WindowPosition::BottomLeft { margin };
        self
    }

    /// Return the requested window size.
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Return the semantic placement.
    pub fn position(&self) -> &WindowPosition {
        &self.position
    }

    /// Validate the placement request before resolving screen bounds.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.size.width > Pixels::ZERO && self.size.height > Pixels::ZERO,
            "window placement size must be greater than zero"
        );
        Ok(())
    }
}

/// Resolved desktop placement for a window or panel.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowPlacement {
    size: Size<Pixels>,
    position: WindowPosition,
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
}

impl WindowPlacement {
    /// Requested window size.
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Semantic placement used to compute the bounds.
    pub fn position(&self) -> &WindowPosition {
        &self.position
    }

    /// Computed screen-space bounds.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Display containing the resolved placement center, when known.
    pub fn display_id(&self) -> Option<DisplayId> {
        self.display_id
    }
}

/// Which display or displays a query should return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayQueryTarget {
    /// Return every active display.
    All,
    /// Return the primary display.
    Primary,
    /// Return the display containing the cursor.
    Cursor,
    /// Return one display by id.
    Display(DisplayId),
}

/// Immutable display information for screen-aware app logic.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySnapshot {
    id: DisplayId,
    uuid: Option<uuid::Uuid>,
    bounds: Bounds<Pixels>,
    default_bounds: Bounds<Pixels>,
    refresh_rate: Option<f32>,
    primary: bool,
    contains_cursor: bool,
}

impl DisplaySnapshot {
    /// Platform display id.
    pub fn id(&self) -> DisplayId {
        self.id
    }

    /// Stable platform display UUID, when available.
    pub fn uuid(&self) -> Option<uuid::Uuid> {
        self.uuid
    }

    /// Screen-space display bounds.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Default bounds used for a normal new window on this display.
    pub fn default_bounds(&self) -> Bounds<Pixels> {
        self.default_bounds
    }

    /// Reported refresh rate in hertz, when available.
    pub fn refresh_rate(&self) -> Option<f32> {
        self.refresh_rate
    }

    /// Whether this is the primary display.
    pub fn is_primary(&self) -> bool {
        self.primary
    }

    /// Whether this display contains the current cursor position.
    pub fn contains_cursor(&self) -> bool {
        self.contains_cursor
    }
}

/// Resolved display query result.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayQueryResult {
    target: DisplayQueryTarget,
    displays: Vec<DisplaySnapshot>,
    cursor_position: Option<Point<Pixels>>,
}

impl DisplayQueryResult {
    /// Requested query target.
    pub fn target(&self) -> DisplayQueryTarget {
        self.target
    }

    /// Matching display snapshots.
    pub fn displays(&self) -> &[DisplaySnapshot] {
        &self.displays
    }

    /// First matching display, useful for primary/cursor/id queries.
    pub fn first(&self) -> Option<&DisplaySnapshot> {
        self.displays.first()
    }

    /// Current cursor position used by the query, when available.
    pub fn cursor_position(&self) -> Option<Point<Pixels>> {
        self.cursor_position
    }

    /// Whether the query found at least one display.
    pub fn has_match(&self) -> bool {
        !self.displays.is_empty()
    }
}

/// Builder for Electron `screen`-style display queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayQueryBuilder {
    target: DisplayQueryTarget,
    require_match: bool,
    fallback_to_primary: bool,
}

impl DisplayQueryBuilder {
    /// Query all active displays.
    pub fn all() -> Self {
        Self {
            target: DisplayQueryTarget::All,
            require_match: false,
            fallback_to_primary: false,
        }
    }

    /// Query the primary display.
    pub fn primary() -> Self {
        Self {
            target: DisplayQueryTarget::Primary,
            require_match: true,
            fallback_to_primary: false,
        }
    }

    /// Query the display containing the cursor.
    pub fn cursor() -> Self {
        Self {
            target: DisplayQueryTarget::Cursor,
            require_match: true,
            fallback_to_primary: true,
        }
    }

    /// Query one display by id.
    pub fn display_id(id: DisplayId) -> Self {
        Self {
            target: DisplayQueryTarget::Display(id),
            require_match: true,
            fallback_to_primary: false,
        }
    }

    /// Require at least one matching display.
    pub fn require_match(mut self) -> Self {
        self.require_match = true;
        self
    }

    /// Allow an empty result instead of returning an error.
    pub fn allow_empty(mut self) -> Self {
        self.require_match = false;
        self
    }

    /// Fall back to the primary display when cursor/id lookup has no match.
    pub fn fallback_to_primary(mut self) -> Self {
        self.fallback_to_primary = true;
        self
    }

    /// Disable primary-display fallback.
    pub fn without_fallback(mut self) -> Self {
        self.fallback_to_primary = false;
        self
    }

    /// Return the configured target.
    pub fn target(&self) -> DisplayQueryTarget {
        self.target
    }

    /// Whether an empty result is an error.
    pub fn requires_match(&self) -> bool {
        self.require_match
    }

    /// Whether the query may fall back to the primary display.
    pub fn falls_back_to_primary(&self) -> bool {
        self.fallback_to_primary
    }

    /// Validate the display query.
    pub fn validate(&self) -> Result<()> {
        if matches!(self.target, DisplayQueryTarget::All) {
            anyhow::ensure!(
                !self.fallback_to_primary,
                "all-display queries cannot fall back to primary display"
            );
        }
        Ok(())
    }
}

/// Builder for monitoring online/offline network status changes.
#[derive(Default)]
pub struct NetworkStatusMonitorBuilder {
    on_change: Option<NetworkStatusCallback>,
    on_online: Option<NetworkTransitionCallback>,
    on_offline: Option<NetworkTransitionCallback>,
}

impl NetworkStatusMonitorBuilder {
    /// Create an empty network monitor builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle every network status change.
    pub fn on_change(mut self, callback: impl FnMut(NetworkStatus, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(callback));
        self
    }

    /// Handle transitions to online.
    pub fn on_online(mut self, callback: impl FnMut(&mut App) + 'static) -> Self {
        self.on_online = Some(Box::new(callback));
        self
    }

    /// Handle transitions to offline.
    pub fn on_offline(mut self, callback: impl FnMut(&mut App) + 'static) -> Self {
        self.on_offline = Some(Box::new(callback));
        self
    }

    /// Return true when any callback has been configured.
    pub fn has_callbacks(&self) -> bool {
        self.on_change.is_some() || self.on_online.is_some() || self.on_offline.is_some()
    }

    /// Validate that the monitor has at least one callback.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.has_callbacks(),
            "network status monitor must configure at least one callback"
        );
        Ok(())
    }

    fn into_parts(
        self,
    ) -> (
        Option<NetworkStatusCallback>,
        Option<NetworkTransitionCallback>,
        Option<NetworkTransitionCallback>,
    ) {
        (self.on_change, self.on_online, self.on_offline)
    }
}

/// Installed network-status monitor snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkStatusMonitor {
    initial_status: NetworkStatus,
}

impl NetworkStatusMonitor {
    /// Network status when the monitor was installed.
    pub fn initial_status(&self) -> NetworkStatus {
        self.initial_status
    }

    /// Return true if the app was online when the monitor was installed.
    pub fn initially_online(&self) -> bool {
        self.initial_status == NetworkStatus::Online
    }

    /// Return true if the app was offline when the monitor was installed.
    pub fn initially_offline(&self) -> bool {
        self.initial_status == NetworkStatus::Offline
    }
}

/// Builder for registering one or more custom URL schemes.
#[derive(Debug, Clone, Default)]
pub struct UrlSchemeRegistrationBuilder {
    schemes: Vec<String>,
}

impl UrlSchemeRegistrationBuilder {
    /// Create an empty URL scheme registration builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one URL scheme, such as `myapp` for `myapp://`.
    pub fn scheme(mut self, scheme: impl Into<String>) -> Self {
        let scheme = scheme.into();
        if !self.schemes.iter().any(|existing| existing == &scheme) {
            self.schemes.push(scheme);
        }
        self
    }

    /// Add multiple URL schemes.
    pub fn schemes(mut self, schemes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for scheme in schemes {
            self = self.scheme(scheme);
        }
        self
    }

    /// Returns the configured schemes.
    pub fn configured_schemes(&self) -> &[String] {
        &self.schemes
    }

    /// Validate every configured scheme.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.schemes.is_empty(),
            "at least one URL scheme must be configured"
        );

        for scheme in &self.schemes {
            validate_url_scheme(scheme)?;
        }

        Ok(())
    }

    /// Build the validated scheme list.
    pub fn build(self) -> Result<Vec<String>> {
        self.validate()?;
        Ok(self.schemes)
    }
}

impl From<String> for UrlSchemeRegistrationBuilder {
    fn from(value: String) -> Self {
        Self::new().scheme(value)
    }
}

impl From<&str> for UrlSchemeRegistrationBuilder {
    fn from(value: &str) -> Self {
        Self::new().scheme(value)
    }
}

/// How strongly an app claims a file association.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAssociationRole {
    /// The app can view/open this type.
    Viewer,
    /// The app edits or owns documents of this type.
    Editor,
}

/// One checked file association declaration for packaging/installers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAssociation {
    name: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
    role: FileAssociationRole,
    description: Option<String>,
}

impl FileAssociation {
    /// User-facing association name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Normalized lowercase file extensions without leading dots.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// MIME types associated with this document type.
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    /// App role for this association.
    pub fn role(&self) -> FileAssociationRole {
        self.role
    }

    /// Optional user-facing description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// Builder for one file association declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAssociationBuilder {
    name: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
    role: FileAssociationRole,
    description: Option<String>,
}

impl FileAssociationBuilder {
    /// Create an association with a user-facing name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            extensions: Vec::new(),
            mime_types: Vec::new(),
            role: FileAssociationRole::Viewer,
            description: None,
        }
    }

    /// Add one file extension, with or without a leading dot.
    pub fn extension(mut self, extension: impl AsRef<str>) -> Self {
        let extension = normalize_drop_extension(extension.as_ref());
        if !extension.is_empty() && !self.extensions.contains(&extension) {
            self.extensions.push(extension);
        }
        self
    }

    /// Add multiple file extensions.
    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        for extension in extensions {
            self = self.extension(extension);
        }
        self
    }

    /// Add one MIME type.
    pub fn mime_type(mut self, mime_type: impl Into<String>) -> Self {
        let mime_type = mime_type.into();
        if !self
            .mime_types
            .iter()
            .any(|existing| existing == &mime_type)
        {
            self.mime_types.push(mime_type);
        }
        self
    }

    /// Add multiple MIME types.
    pub fn mime_types(mut self, mime_types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for mime_type in mime_types {
            self = self.mime_type(mime_type);
        }
        self
    }

    /// Mark this app as an editor/owner for this document type.
    pub fn editor(mut self) -> Self {
        self.role = FileAssociationRole::Editor;
        self
    }

    /// Mark this app as a viewer for this document type.
    pub fn viewer(mut self) -> Self {
        self.role = FileAssociationRole::Viewer;
        self
    }

    /// Set a user-facing association description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Validate this association declaration.
    pub fn validate(&self) -> Result<()> {
        validate_app_metadata_text(&self.name, "file association name", 128, false)?;
        anyhow::ensure!(
            !self.extensions.is_empty() || !self.mime_types.is_empty(),
            "file association must include at least one extension or MIME type"
        );
        for extension in &self.extensions {
            validate_file_association_extension(extension)?;
        }
        for mime_type in &self.mime_types {
            validate_mime_type(mime_type, "file association MIME type")?;
        }
        if let Some(description) = &self.description {
            validate_app_metadata_text(description, "file association description", 256, true)?;
        }
        Ok(())
    }

    /// Build a checked file association.
    pub fn build_checked(self) -> Result<FileAssociation> {
        self.validate()?;
        Ok(FileAssociation {
            name: self.name,
            extensions: self.extensions,
            mime_types: self.mime_types,
            role: self.role,
            description: self.description,
        })
    }
}

/// Checked app file-association declaration set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAssociationSet {
    associations: Vec<FileAssociation>,
}

impl FileAssociationSet {
    /// Checked file associations.
    pub fn associations(&self) -> &[FileAssociation] {
        &self.associations
    }

    /// Whether any association declares this extension.
    pub fn accepts_extension(&self, extension: &str) -> bool {
        let extension = normalize_drop_extension(extension);
        self.associations
            .iter()
            .any(|association| association.extensions.contains(&extension))
    }

    /// Whether any association declares this MIME type.
    pub fn accepts_mime_type(&self, mime_type: &str) -> bool {
        self.associations.iter().any(|association| {
            association
                .mime_types
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(mime_type))
        })
    }
}

fn file_association_builder_from_checked(association: &FileAssociation) -> FileAssociationBuilder {
    let mut builder = FileAssociationBuilder::new(association.name.clone())
        .extensions(association.extensions.clone())
        .mime_types(association.mime_types.clone());
    builder = match association.role {
        FileAssociationRole::Viewer => builder.viewer(),
        FileAssociationRole::Editor => builder.editor(),
    };
    if let Some(description) = &association.description {
        builder = builder.description(description.clone());
    }
    builder
}

/// Builder for app-level file association declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileAssociationSetBuilder {
    associations: Vec<FileAssociationBuilder>,
}

impl FileAssociationSetBuilder {
    /// Create an empty association set builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one association.
    pub fn association(mut self, association: FileAssociationBuilder) -> Self {
        self.associations.push(association);
        self
    }

    /// Add multiple associations.
    pub fn associations(
        mut self,
        associations: impl IntoIterator<Item = FileAssociationBuilder>,
    ) -> Self {
        self.associations.extend(associations);
        self
    }

    /// Return configured association builders.
    pub fn configured_associations(&self) -> &[FileAssociationBuilder] {
        &self.associations
    }

    /// Validate every association and prevent duplicate extensions/MIME types.
    pub fn validate(&self) -> Result<()> {
        self.build_associations().map(|_| ())
    }

    /// Build the checked association set.
    pub fn build_checked(self) -> Result<FileAssociationSet> {
        Ok(FileAssociationSet {
            associations: self.build_associations()?,
        })
    }

    fn build_associations(&self) -> Result<Vec<FileAssociation>> {
        anyhow::ensure!(
            !self.associations.is_empty(),
            "file association set must include at least one association"
        );
        let mut associations = Vec::with_capacity(self.associations.len());
        let mut extensions = Vec::<String>::new();
        let mut mime_types = Vec::<String>::new();
        for association in &self.associations {
            association.validate()?;
            for extension in &association.extensions {
                anyhow::ensure!(
                    !extensions.contains(extension),
                    "file association extension declared more than once: {extension}"
                );
                extensions.push(extension.clone());
            }
            for mime_type in &association.mime_types {
                let normalized = mime_type.to_ascii_lowercase();
                anyhow::ensure!(
                    !mime_types.contains(&normalized),
                    "file association MIME type declared more than once: {mime_type}"
                );
                mime_types.push(normalized);
            }
            associations.push(association.clone().build_checked()?);
        }
        Ok(associations)
    }
}

/// Whether a default-handler registration plan targets the current user or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultHandlerScope {
    /// Register for the current user profile only.
    CurrentUser,
    /// Register system-wide, typically requiring installer/admin privileges.
    System,
}

/// Checked intent for making this app a default handler for schemes or documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultHandlerPlan {
    app_id: String,
    app_name: Option<String>,
    url_schemes: Vec<String>,
    file_associations: Vec<FileAssociation>,
    scope: DefaultHandlerScope,
    require_user_confirmation: bool,
}

impl DefaultHandlerPlan {
    /// Stable app/package identifier used by platform registration.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Optional user-facing app name for setup prompts.
    pub fn app_name(&self) -> Option<&str> {
        self.app_name.as_deref()
    }

    /// URL schemes this app wants to handle by default.
    pub fn url_schemes(&self) -> &[String] {
        &self.url_schemes
    }

    /// Document/file types this app wants to handle by default.
    pub fn file_associations(&self) -> &[FileAssociation] {
        &self.file_associations
    }

    /// Registration scope requested by the plan.
    pub fn scope(&self) -> DefaultHandlerScope {
        self.scope
    }

    /// Whether callers should show an explicit user confirmation before OS mutation.
    pub fn requires_user_confirmation(&self) -> bool {
        self.require_user_confirmation
    }

    /// Whether the plan claims a URL scheme.
    pub fn claims_scheme(&self, scheme: &str) -> bool {
        self.url_schemes
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(scheme))
    }

    /// Whether the plan claims a file extension.
    pub fn claims_extension(&self, extension: &str) -> bool {
        let extension = normalize_drop_extension(extension);
        self.file_associations
            .iter()
            .any(|association| association.extensions.contains(&extension))
    }

    /// Whether the plan claims a MIME type.
    pub fn claims_mime_type(&self, mime_type: &str) -> bool {
        self.file_associations.iter().any(|association| {
            association
                .mime_types
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(mime_type))
        })
    }
}

/// Builder for checked default-handler registration intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultHandlerPlanBuilder {
    app_id: String,
    app_name: Option<String>,
    url_schemes: Vec<String>,
    file_associations: Vec<FileAssociationBuilder>,
    scope: DefaultHandlerScope,
    require_user_confirmation: bool,
}

impl DefaultHandlerPlanBuilder {
    /// Create a default-handler plan for a stable app/package identifier.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            app_name: None,
            url_schemes: Vec::new(),
            file_associations: Vec::new(),
            scope: DefaultHandlerScope::CurrentUser,
            require_user_confirmation: true,
        }
    }

    /// Seed a plan from checked package metadata.
    pub fn from_package_manifest(manifest: &AppPackageManifest) -> Self {
        let app_id = manifest
            .metadata()
            .identifier()
            .unwrap_or_else(|| manifest.metadata().name());
        let mut builder = Self::new(app_id).app_name(manifest.metadata().name());
        for scheme in manifest.url_schemes() {
            builder = builder.scheme(scheme.clone());
        }
        for association in manifest.file_associations() {
            builder = builder.file_association(file_association_builder_from_checked(association));
        }
        builder
    }

    /// Set a user-facing app name for setup prompts.
    pub fn app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = Some(name.into());
        self
    }

    /// Add one URL scheme claim.
    pub fn scheme(mut self, scheme: impl Into<String>) -> Self {
        self.url_schemes.push(scheme.into());
        self
    }

    /// Add multiple URL scheme claims.
    pub fn schemes(mut self, schemes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.url_schemes.extend(schemes.into_iter().map(Into::into));
        self
    }

    /// Add URL schemes from the existing registration builder.
    pub fn url_schemes(mut self, schemes: impl Into<UrlSchemeRegistrationBuilder>) -> Self {
        self.url_schemes
            .extend(schemes.into().configured_schemes().iter().cloned());
        self
    }

    /// Add one file association claim.
    pub fn file_association(mut self, association: FileAssociationBuilder) -> Self {
        self.file_associations.push(association);
        self
    }

    /// Add multiple file association claims.
    pub fn file_associations(mut self, associations: FileAssociationSetBuilder) -> Self {
        self.file_associations
            .extend(associations.configured_associations().iter().cloned());
        self
    }

    /// Request current-user registration.
    pub fn current_user_scope(mut self) -> Self {
        self.scope = DefaultHandlerScope::CurrentUser;
        self
    }

    /// Request system-wide registration.
    pub fn system_scope(mut self) -> Self {
        self.scope = DefaultHandlerScope::System;
        self
    }

    /// Set whether setup should require explicit user confirmation.
    pub fn require_user_confirmation(mut self, required: bool) -> Self {
        self.require_user_confirmation = required;
        self
    }

    /// Validate default-handler intent without mutating OS defaults.
    pub fn validate(&self) -> Result<()> {
        self.clone().build_checked().map(|_| ())
    }

    /// Build the checked default-handler plan.
    pub fn build_checked(self) -> Result<DefaultHandlerPlan> {
        validate_app_path_id(&self.app_id).context("invalid default handler app id")?;
        if let Some(app_name) = &self.app_name {
            validate_app_metadata_text(app_name, "default handler app name", 128, false)?;
        }
        anyhow::ensure!(
            !self.url_schemes.is_empty() || !self.file_associations.is_empty(),
            "default handler plan must claim at least one URL scheme or file association"
        );

        let mut schemes = Vec::new();
        for scheme in self.url_schemes {
            validate_url_scheme(&scheme)?;
            let normalized = scheme.to_ascii_lowercase();
            anyhow::ensure!(
                !schemes.contains(&normalized),
                "default handler URL scheme declared more than once: {scheme}"
            );
            schemes.push(normalized);
        }

        let associations = if self.file_associations.is_empty() {
            Vec::new()
        } else {
            FileAssociationSetBuilder::new()
                .associations(self.file_associations)
                .build_checked()?
                .associations
        };

        Ok(DefaultHandlerPlan {
            app_id: self.app_id,
            app_name: self.app_name,
            url_schemes: schemes,
            file_associations: associations,
            scope: self.scope,
            require_user_confirmation: self.require_user_confirmation,
        })
    }
}

/// Checked packaging metadata for bundlers, installers, and generated docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPackageManifest {
    metadata: AppMetadata,
    url_schemes: Vec<String>,
    file_associations: Vec<FileAssociation>,
    icons: Vec<AppIconAsset>,
    privacy_permissions: Vec<AppPrivacyPermission>,
}

impl AppPackageManifest {
    /// Validated app identity metadata.
    pub fn metadata(&self) -> &AppMetadata {
        &self.metadata
    }

    /// Runtime/deep-link URL schemes the app claims.
    pub fn url_schemes(&self) -> &[String] {
        &self.url_schemes
    }

    /// Document/file associations the app claims.
    pub fn file_associations(&self) -> &[FileAssociation] {
        &self.file_associations
    }

    /// Icon assets declared for packaging and native chrome.
    pub fn icons(&self) -> &[AppIconAsset] {
        &self.icons
    }

    /// Icons matching one semantic purpose.
    pub fn icons_for(&self, purpose: AppIconPurpose) -> Vec<&AppIconAsset> {
        self.icons
            .iter()
            .filter(|icon| icon.purpose == purpose)
            .collect()
    }

    /// Privacy permission declarations for packaging.
    pub fn privacy_permissions(&self) -> &[AppPrivacyPermission] {
        &self.privacy_permissions
    }

    /// macOS usage-description entries derived from privacy declarations.
    pub fn macos_usage_descriptions(&self) -> Vec<MacOsUsageDescription> {
        AppPrivacyManifest {
            permissions: self.privacy_permissions.clone(),
        }
        .macos_usage_descriptions()
    }

    /// macOS `CFBundleURLTypes`-shaped declarations.
    pub fn macos_url_types(&self) -> Vec<MacOsUrlTypeDeclaration> {
        if self.url_schemes.is_empty() {
            return Vec::new();
        }

        vec![MacOsUrlTypeDeclaration {
            name: self
                .metadata
                .identifier()
                .unwrap_or(self.metadata.name())
                .to_string(),
            schemes: self.url_schemes.clone(),
        }]
    }

    /// macOS `CFBundleDocumentTypes`-shaped declarations.
    pub fn macos_document_types(&self) -> Vec<MacOsDocumentTypeDeclaration> {
        self.file_associations
            .iter()
            .map(|association| MacOsDocumentTypeDeclaration {
                name: association.name.clone(),
                extensions: association.extensions.clone(),
                mime_types: association.mime_types.clone(),
                role: association.role,
            })
            .collect()
    }

    /// Linux `.desktop` `MimeType=` entries from declared MIME types.
    pub fn linux_desktop_mime_types(&self) -> Vec<String> {
        let mut mime_types = Vec::new();
        for association in &self.file_associations {
            for mime_type in &association.mime_types {
                if !mime_types
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(mime_type))
                {
                    mime_types.push(mime_type.clone());
                }
            }
        }
        mime_types
    }

    /// Windows file association declarations with stable ProgIDs.
    pub fn windows_file_associations(&self) -> Vec<WindowsFileAssociationDeclaration> {
        let identifier = self.metadata.identifier().unwrap_or(self.metadata.name());
        self.file_associations
            .iter()
            .map(|association| WindowsFileAssociationDeclaration {
                name: association.name.clone(),
                extensions: association.extensions.clone(),
                mime_types: association.mime_types.clone(),
                prog_id: format!(
                    "{}.{}",
                    sanitize_windows_progid_component(identifier),
                    sanitize_windows_progid_component(&association.name)
                ),
                role: association.role,
                description: association.description.clone(),
            })
            .collect()
    }

    /// Whether this package manifest claims the given extension.
    pub fn accepts_extension(&self, extension: &str) -> bool {
        let extension = normalize_drop_extension(extension);
        self.file_associations
            .iter()
            .any(|association| association.extensions.contains(&extension))
    }

    /// Whether this package manifest claims the given MIME type.
    pub fn accepts_mime_type(&self, mime_type: &str) -> bool {
        self.file_associations.iter().any(|association| {
            association
                .mime_types
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(mime_type))
        })
    }

    /// Evaluate this manifest for packaging readiness.
    pub fn readiness_report(&self) -> AppPackageReadinessReport {
        AppPackageReadinessBuilder::new(self.clone()).evaluate()
    }
}

/// Builder for checked package metadata that composes identity, URL schemes, and file associations.
#[derive(Debug, Clone)]
pub struct AppPackageManifestBuilder {
    metadata: AppMetadataBuilder,
    url_schemes: UrlSchemeRegistrationBuilder,
    file_associations: Option<FileAssociationSetBuilder>,
    icons: Option<AppIconSetBuilder>,
    privacy_permissions: Option<AppPrivacyManifestBuilder>,
}

impl AppPackageManifestBuilder {
    /// Create package metadata from app identity.
    pub fn new(metadata: AppMetadataBuilder) -> Self {
        Self {
            metadata,
            url_schemes: UrlSchemeRegistrationBuilder::new(),
            file_associations: None,
            icons: None,
            privacy_permissions: None,
        }
    }

    /// Add URL scheme declarations.
    pub fn url_schemes(mut self, schemes: impl Into<UrlSchemeRegistrationBuilder>) -> Self {
        self.url_schemes = schemes.into();
        self
    }

    /// Add file association declarations.
    pub fn file_associations(mut self, associations: FileAssociationSetBuilder) -> Self {
        self.file_associations = Some(associations);
        self
    }

    /// Add icon asset declarations.
    pub fn icons(mut self, icons: AppIconSetBuilder) -> Self {
        self.icons = Some(icons);
        self
    }

    /// Add privacy permission declarations.
    pub fn privacy_permissions(mut self, permissions: AppPrivacyManifestBuilder) -> Self {
        self.privacy_permissions = Some(permissions);
        self
    }

    /// Validate the package manifest declaration.
    pub fn validate(&self) -> Result<()> {
        let metadata = self.metadata.clone().build_checked()?;
        anyhow::ensure!(
            metadata.identifier().is_some(),
            "package manifest metadata must include an app identifier"
        );
        if !self.url_schemes.configured_schemes().is_empty() {
            self.url_schemes.validate()?;
        }
        if let Some(associations) = &self.file_associations {
            associations.validate()?;
        }
        if let Some(icons) = &self.icons {
            icons.validate()?;
        }
        if let Some(permissions) = &self.privacy_permissions {
            permissions.validate()?;
        }
        Ok(())
    }

    /// Build checked package metadata for bundlers and installers.
    pub fn build_checked(self) -> Result<AppPackageManifest> {
        self.validate()?;
        let metadata = self.metadata.build_checked()?;
        let url_schemes = if self.url_schemes.configured_schemes().is_empty() {
            Vec::new()
        } else {
            self.url_schemes.build()?
        };
        let file_associations = match self.file_associations {
            Some(associations) => associations.build_checked()?.associations,
            None => Vec::new(),
        };
        let icons = match self.icons {
            Some(icons) => icons.build_checked()?.icons,
            None => Vec::new(),
        };
        let privacy_permissions = match self.privacy_permissions {
            Some(permissions) => permissions.build_checked()?.permissions,
            None => Vec::new(),
        };

        Ok(AppPackageManifest {
            metadata,
            url_schemes,
            file_associations,
            icons,
            privacy_permissions,
        })
    }
}

/// Severity for a package readiness finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPackageReadinessSeverity {
    /// Blocks a normal release/package.
    Error,
    /// Does not block packaging, but should be reviewed before shipping.
    Warning,
}

/// Kind of package readiness finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPackageReadinessIssueKind {
    /// App metadata has no version.
    MissingVersion,
    /// No primary app icon was declared.
    MissingAppIcon,
    /// File associations were declared without a document icon.
    MissingDocumentIcon,
    /// A file association has extensions but no MIME type metadata.
    MissingFileAssociationMimeType,
    /// A privacy declaration has no known platform usage-description export.
    PrivacyDeclarationWithoutUsageDescription,
}

/// One package readiness finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPackageReadinessIssue {
    severity: AppPackageReadinessSeverity,
    kind: AppPackageReadinessIssueKind,
    message: String,
}

impl AppPackageReadinessIssue {
    /// Finding severity.
    pub fn severity(&self) -> AppPackageReadinessSeverity {
        self.severity
    }

    /// Finding kind.
    pub fn kind(&self) -> AppPackageReadinessIssueKind {
        self.kind
    }

    /// Human-readable finding message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Package readiness report for generated apps, release scripts, and agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPackageReadinessReport {
    manifest: AppPackageManifest,
    issues: Vec<AppPackageReadinessIssue>,
}

impl AppPackageReadinessReport {
    /// Manifest that was evaluated.
    pub fn manifest(&self) -> &AppPackageManifest {
        &self.manifest
    }

    /// All readiness findings.
    pub fn issues(&self) -> &[AppPackageReadinessIssue] {
        &self.issues
    }

    /// Blocking findings only.
    pub fn errors(&self) -> Vec<&AppPackageReadinessIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == AppPackageReadinessSeverity::Error)
            .collect()
    }

    /// Non-blocking findings only.
    pub fn warnings(&self) -> Vec<&AppPackageReadinessIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == AppPackageReadinessSeverity::Warning)
            .collect()
    }

    /// Whether the manifest passes blocking release checks.
    pub fn is_ready(&self) -> bool {
        self.errors().is_empty()
    }

    /// Compact human-readable summary.
    pub fn summary(&self) -> String {
        if self.issues.is_empty() {
            return "package manifest is ready".to_string();
        }

        let errors = self.errors().len();
        let warnings = self.warnings().len();
        format!("package manifest has {errors} error(s) and {warnings} warning(s)")
    }
}

/// Builder for package readiness checks.
#[derive(Debug, Clone)]
pub struct AppPackageReadinessBuilder {
    manifest: AppPackageManifest,
    require_version: bool,
    require_app_icon: bool,
    warn_document_icon: bool,
    warn_file_association_mime_types: bool,
    warn_privacy_platform_exports: bool,
}

impl AppPackageReadinessBuilder {
    /// Create a readiness check for a checked package manifest.
    pub fn new(manifest: AppPackageManifest) -> Self {
        Self {
            manifest,
            require_version: true,
            require_app_icon: true,
            warn_document_icon: true,
            warn_file_association_mime_types: true,
            warn_privacy_platform_exports: true,
        }
    }

    /// Do not require metadata version.
    pub fn allow_missing_version(mut self) -> Self {
        self.require_version = false;
        self
    }

    /// Do not require a primary app icon.
    pub fn allow_missing_app_icon(mut self) -> Self {
        self.require_app_icon = false;
        self
    }

    /// Do not warn when document associations lack a document icon.
    pub fn allow_missing_document_icon(mut self) -> Self {
        self.warn_document_icon = false;
        self
    }

    /// Do not warn when file associations lack MIME metadata.
    pub fn allow_extension_only_file_associations(mut self) -> Self {
        self.warn_file_association_mime_types = false;
        self
    }

    /// Do not warn when privacy declarations have no known platform usage-description export.
    pub fn allow_privacy_declarations_without_platform_exports(mut self) -> Self {
        self.warn_privacy_platform_exports = false;
        self
    }

    /// Evaluate package readiness.
    pub fn evaluate(self) -> AppPackageReadinessReport {
        let mut issues = Vec::new();

        if self.require_version && self.manifest.metadata.version().is_none() {
            issues.push(package_readiness_issue(
                AppPackageReadinessSeverity::Error,
                AppPackageReadinessIssueKind::MissingVersion,
                "package manifest metadata should include an app version",
            ));
        }

        if self.require_app_icon
            && !self
                .manifest
                .icons
                .iter()
                .any(|icon| icon.purpose == AppIconPurpose::App)
        {
            issues.push(package_readiness_issue(
                AppPackageReadinessSeverity::Error,
                AppPackageReadinessIssueKind::MissingAppIcon,
                "package manifest should include a primary app icon",
            ));
        }

        if self.warn_document_icon
            && !self.manifest.file_associations.is_empty()
            && !self
                .manifest
                .icons
                .iter()
                .any(|icon| icon.purpose == AppIconPurpose::Document)
        {
            issues.push(package_readiness_issue(
                AppPackageReadinessSeverity::Warning,
                AppPackageReadinessIssueKind::MissingDocumentIcon,
                "file associations were declared without a document icon",
            ));
        }

        if self.warn_file_association_mime_types {
            for association in &self.manifest.file_associations {
                if !association.extensions.is_empty() && association.mime_types.is_empty() {
                    issues.push(package_readiness_issue(
                        AppPackageReadinessSeverity::Warning,
                        AppPackageReadinessIssueKind::MissingFileAssociationMimeType,
                        format!(
                            "file association `{}` has extensions but no MIME type metadata",
                            association.name()
                        ),
                    ));
                }
            }
        }

        if self.warn_privacy_platform_exports {
            for permission in &self.manifest.privacy_permissions {
                if permission.macos_usage_description().is_none() {
                    issues.push(package_readiness_issue(
                        AppPackageReadinessSeverity::Warning,
                        AppPackageReadinessIssueKind::PrivacyDeclarationWithoutUsageDescription,
                        format!(
                            "privacy permission `{}` has no known macOS usage-description key",
                            permission.kind().key()
                        ),
                    ));
                }
            }
        }

        AppPackageReadinessReport {
            manifest: self.manifest,
            issues,
        }
    }
}

fn package_readiness_issue(
    severity: AppPackageReadinessSeverity,
    kind: AppPackageReadinessIssueKind,
    message: impl Into<String>,
) -> AppPackageReadinessIssue {
    AppPackageReadinessIssue {
        severity,
        kind,
        message: message.into(),
    }
}

/// Operating system target for package artifact generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppDistributionPlatform {
    /// macOS app distribution.
    MacOs,
    /// Windows app distribution.
    Windows,
    /// Linux app distribution.
    Linux,
}

impl AppDistributionPlatform {
    /// Stable platform key.
    pub fn key(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

/// Package artifact format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppDistributionFormat {
    /// macOS disk image.
    Dmg,
    /// macOS zip archive.
    MacZip,
    /// Windows MSI installer.
    Msi,
    /// Windows NSIS-style executable installer.
    Nsis,
    /// Linux AppImage.
    AppImage,
    /// Debian package.
    Deb,
    /// RPM package.
    Rpm,
    /// Generic tar archive.
    TarGz,
}

impl AppDistributionFormat {
    /// Platform supported by this artifact format.
    pub fn platform(self) -> AppDistributionPlatform {
        match self {
            Self::Dmg | Self::MacZip => AppDistributionPlatform::MacOs,
            Self::Msi | Self::Nsis => AppDistributionPlatform::Windows,
            Self::AppImage | Self::Deb | Self::Rpm | Self::TarGz => AppDistributionPlatform::Linux,
        }
    }

    /// Stable format key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Dmg => "dmg",
            Self::MacZip => "mac-zip",
            Self::Msi => "msi",
            Self::Nsis => "nsis",
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::TarGz => "tar-gz",
        }
    }

    /// File extension normally produced for this artifact.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Dmg => "dmg",
            Self::MacZip => "zip",
            Self::Msi => "msi",
            Self::Nsis => "exe",
            Self::AppImage => "AppImage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::TarGz => "tar.gz",
        }
    }
}

/// One checked distribution artifact target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDistributionTarget {
    format: AppDistributionFormat,
    channel: Option<String>,
}

impl AppDistributionTarget {
    /// Artifact format.
    pub fn format(&self) -> AppDistributionFormat {
        self.format
    }

    /// Target platform.
    pub fn platform(&self) -> AppDistributionPlatform {
        self.format.platform()
    }

    /// Optional release channel label.
    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// Default artifact filename for this target and manifest.
    pub fn artifact_file_name(&self, manifest: &AppPackageManifest) -> String {
        let name = sanitize_artifact_name(manifest.metadata().name());
        let version = manifest.metadata().version().unwrap_or("dev");
        let channel = self
            .channel
            .as_deref()
            .map(|channel| format!("-{}", sanitize_artifact_name(channel)))
            .unwrap_or_default();
        format!(
            "{name}-{version}{channel}-{}.{}",
            self.platform().key(),
            self.format.extension()
        )
    }
}

/// Builder for one distribution target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDistributionTargetBuilder {
    format: AppDistributionFormat,
    channel: Option<String>,
}

impl AppDistributionTargetBuilder {
    /// macOS DMG target.
    pub fn dmg() -> Self {
        Self::new(AppDistributionFormat::Dmg)
    }

    /// macOS zip target.
    pub fn mac_zip() -> Self {
        Self::new(AppDistributionFormat::MacZip)
    }

    /// Windows MSI target.
    pub fn msi() -> Self {
        Self::new(AppDistributionFormat::Msi)
    }

    /// Windows NSIS installer target.
    pub fn nsis() -> Self {
        Self::new(AppDistributionFormat::Nsis)
    }

    /// Linux AppImage target.
    pub fn appimage() -> Self {
        Self::new(AppDistributionFormat::AppImage)
    }

    /// Debian package target.
    pub fn deb() -> Self {
        Self::new(AppDistributionFormat::Deb)
    }

    /// RPM package target.
    pub fn rpm() -> Self {
        Self::new(AppDistributionFormat::Rpm)
    }

    /// Linux tar.gz target.
    pub fn tar_gz() -> Self {
        Self::new(AppDistributionFormat::TarGz)
    }

    /// Create a distribution target for a format.
    pub fn new(format: AppDistributionFormat) -> Self {
        Self {
            format,
            channel: None,
        }
    }

    /// Set release channel label.
    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }

    /// Validate this target.
    pub fn validate(&self) -> Result<()> {
        if let Some(channel) = &self.channel {
            validate_app_metadata_text(channel, "distribution channel", 64, false)?;
            anyhow::ensure!(
                channel
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')),
                "distribution channel must contain only ASCII letters, digits, '-', '_', or '.'"
            );
        }
        Ok(())
    }

    /// Build a checked distribution target.
    pub fn build_checked(self) -> Result<AppDistributionTarget> {
        self.validate()?;
        Ok(AppDistributionTarget {
            format: self.format,
            channel: self.channel,
        })
    }
}

/// Checked distribution target plan for release scripts and agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDistributionPlan {
    output_dir: PathBuf,
    targets: Vec<AppDistributionTarget>,
}

impl AppDistributionPlan {
    /// Directory where artifacts should be emitted.
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// Checked artifact targets.
    pub fn targets(&self) -> &[AppDistributionTarget] {
        &self.targets
    }

    /// Targets for one platform.
    pub fn targets_for(&self, platform: AppDistributionPlatform) -> Vec<&AppDistributionTarget> {
        self.targets
            .iter()
            .filter(|target| target.platform() == platform)
            .collect()
    }

    /// Planned artifact paths for a manifest.
    pub fn artifact_paths(&self, manifest: &AppPackageManifest) -> Vec<PathBuf> {
        self.targets
            .iter()
            .map(|target| self.output_dir.join(target.artifact_file_name(manifest)))
            .collect()
    }
}

/// Builder for distribution target plans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDistributionPlanBuilder {
    output_dir: PathBuf,
    targets: Vec<AppDistributionTargetBuilder>,
}

impl AppDistributionPlanBuilder {
    /// Create a distribution plan with an output directory.
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            targets: Vec::new(),
        }
    }

    /// Add one artifact target.
    pub fn target(mut self, target: AppDistributionTargetBuilder) -> Self {
        self.targets.push(target);
        self
    }

    /// Add multiple artifact targets.
    pub fn targets(
        mut self,
        targets: impl IntoIterator<Item = AppDistributionTargetBuilder>,
    ) -> Self {
        self.targets.extend(targets);
        self
    }

    /// Return configured target builders.
    pub fn configured_targets(&self) -> &[AppDistributionTargetBuilder] {
        &self.targets
    }

    /// Validate the distribution plan.
    pub fn validate(&self) -> Result<()> {
        self.build_targets().map(|_| ())
    }

    /// Build a checked distribution plan.
    pub fn build_checked(self) -> Result<AppDistributionPlan> {
        Ok(AppDistributionPlan {
            output_dir: self.validated_output_dir()?,
            targets: self.build_targets()?,
        })
    }

    fn validated_output_dir(&self) -> Result<PathBuf> {
        validate_non_empty_path(&self.output_dir, "distribution output directory")?;
        anyhow::ensure!(
            self.output_dir.is_absolute(),
            "distribution output directory must be absolute"
        );
        Ok(self.output_dir.clone())
    }

    fn build_targets(&self) -> Result<Vec<AppDistributionTarget>> {
        self.validated_output_dir()?;
        anyhow::ensure!(
            !self.targets.is_empty(),
            "distribution plan must include at least one target"
        );

        let mut targets = Vec::with_capacity(self.targets.len());
        for target in &self.targets {
            let target = target.clone().build_checked()?;
            anyhow::ensure!(
                !targets.iter().any(|existing: &AppDistributionTarget| {
                    existing.format == target.format && existing.channel == target.channel
                }),
                "distribution target declared more than once: {}",
                target.format.key()
            );
            targets.push(target);
        }
        Ok(targets)
    }
}

fn sanitize_artifact_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();

    if sanitized.is_empty() {
        "app".to_string()
    } else {
        sanitized
    }
}

/// Signing/notarization declaration for one distribution platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSigningTarget {
    platform: AppDistributionPlatform,
    identity: Option<String>,
    team_id: Option<String>,
    hardened_runtime: bool,
    notarize: bool,
    timestamp: bool,
}

impl AppSigningTarget {
    /// Platform this signing declaration applies to.
    pub fn platform(&self) -> AppDistributionPlatform {
        self.platform
    }

    /// Signing identity, certificate subject, key id, or key alias.
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// Apple Developer Team ID or platform-specific team/account id.
    pub fn team_id(&self) -> Option<&str> {
        self.team_id.as_deref()
    }

    /// Whether macOS hardened runtime should be enabled.
    pub fn hardened_runtime(&self) -> bool {
        self.hardened_runtime
    }

    /// Whether notarization is required after signing.
    pub fn notarize(&self) -> bool {
        self.notarize
    }

    /// Whether signed artifacts should be timestamped.
    pub fn timestamp(&self) -> bool {
        self.timestamp
    }
}

/// Builder for one signing/notarization declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSigningTargetBuilder {
    platform: AppDistributionPlatform,
    identity: Option<String>,
    team_id: Option<String>,
    hardened_runtime: bool,
    notarize: bool,
    timestamp: bool,
}

impl AppSigningTargetBuilder {
    /// macOS Developer ID signing declaration.
    pub fn macos_developer_id(identity: impl Into<String>) -> Self {
        Self::new(AppDistributionPlatform::MacOs).identity(identity)
    }

    /// Windows Authenticode signing declaration.
    pub fn windows_authenticode(identity: impl Into<String>) -> Self {
        Self::new(AppDistributionPlatform::Windows).identity(identity)
    }

    /// Linux package/repository signing declaration.
    pub fn linux_package(identity: impl Into<String>) -> Self {
        Self::new(AppDistributionPlatform::Linux).identity(identity)
    }

    /// Create a signing declaration for a platform.
    pub fn new(platform: AppDistributionPlatform) -> Self {
        Self {
            platform,
            identity: None,
            team_id: None,
            hardened_runtime: false,
            notarize: false,
            timestamp: true,
        }
    }

    /// Set signing identity, certificate subject, key id, or key alias.
    pub fn identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = Some(identity.into());
        self
    }

    /// Set Apple Developer Team ID or platform-specific team/account id.
    pub fn team_id(mut self, team_id: impl Into<String>) -> Self {
        self.team_id = Some(team_id.into());
        self
    }

    /// Enable macOS hardened runtime.
    pub fn hardened_runtime(mut self) -> Self {
        self.hardened_runtime = true;
        self
    }

    /// Require notarization after signing.
    pub fn notarize(mut self) -> Self {
        self.notarize = true;
        self
    }

    /// Disable timestamping for this signing declaration.
    pub fn without_timestamp(mut self) -> Self {
        self.timestamp = false;
        self
    }

    /// Validate this signing declaration.
    pub fn validate(&self) -> Result<()> {
        if let Some(identity) = &self.identity {
            validate_app_metadata_text(identity, "signing identity", 256, false)?;
        }
        if let Some(team_id) = &self.team_id {
            validate_signing_team_id(team_id)?;
        }
        anyhow::ensure!(
            !(self.notarize && self.platform != AppDistributionPlatform::MacOs),
            "notarization is only supported for macOS signing targets"
        );
        anyhow::ensure!(
            !(self.hardened_runtime && self.platform != AppDistributionPlatform::MacOs),
            "hardened runtime is only supported for macOS signing targets"
        );
        anyhow::ensure!(
            !(self.notarize && self.identity.is_none()),
            "notarization requires a macOS signing identity"
        );
        Ok(())
    }

    /// Build a checked signing declaration.
    pub fn build_checked(self) -> Result<AppSigningTarget> {
        self.validate()?;
        Ok(AppSigningTarget {
            platform: self.platform,
            identity: self.identity,
            team_id: self.team_id,
            hardened_runtime: self.hardened_runtime,
            notarize: self.notarize,
            timestamp: self.timestamp,
        })
    }
}

/// Checked signing plan for release scripts and platform bundlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSigningPlan {
    targets: Vec<AppSigningTarget>,
}

impl AppSigningPlan {
    /// Checked signing declarations.
    pub fn targets(&self) -> &[AppSigningTarget] {
        &self.targets
    }

    /// Signing declaration for one platform.
    pub fn target_for(&self, platform: AppDistributionPlatform) -> Option<&AppSigningTarget> {
        self.targets
            .iter()
            .find(|target| target.platform == platform)
    }

    /// Whether all distribution platforms have signing declarations.
    pub fn covers_distribution_plan(&self, plan: &AppDistributionPlan) -> bool {
        plan.targets()
            .iter()
            .all(|target| self.target_for(target.platform()).is_some())
    }
}

/// Builder for grouped signing declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppSigningPlanBuilder {
    targets: Vec<AppSigningTargetBuilder>,
}

impl AppSigningPlanBuilder {
    /// Create an empty signing plan builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one signing declaration.
    pub fn target(mut self, target: AppSigningTargetBuilder) -> Self {
        self.targets.push(target);
        self
    }

    /// Add multiple signing declarations.
    pub fn targets(mut self, targets: impl IntoIterator<Item = AppSigningTargetBuilder>) -> Self {
        self.targets.extend(targets);
        self
    }

    /// Return configured signing declarations.
    pub fn configured_targets(&self) -> &[AppSigningTargetBuilder] {
        &self.targets
    }

    /// Validate every signing declaration.
    pub fn validate(&self) -> Result<()> {
        self.build_targets().map(|_| ())
    }

    /// Build a checked signing plan.
    pub fn build_checked(self) -> Result<AppSigningPlan> {
        Ok(AppSigningPlan {
            targets: self.build_targets()?,
        })
    }

    fn build_targets(&self) -> Result<Vec<AppSigningTarget>> {
        anyhow::ensure!(
            !self.targets.is_empty(),
            "signing plan must include at least one target"
        );
        let mut targets = Vec::with_capacity(self.targets.len());
        for target in &self.targets {
            let target = target.clone().build_checked()?;
            anyhow::ensure!(
                !targets
                    .iter()
                    .any(|existing: &AppSigningTarget| existing.platform == target.platform),
                "signing target declared more than once: {}",
                target.platform.key()
            );
            targets.push(target);
        }
        Ok(targets)
    }
}

fn validate_signing_team_id(team_id: &str) -> Result<()> {
    validate_app_metadata_text(team_id, "signing team id", 64, false)?;
    anyhow::ensure!(
        team_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')),
        "signing team id must contain only ASCII letters, digits, '-', '_', or '.'"
    );
    Ok(())
}

/// Semantic use for a package/runtime icon asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppIconPurpose {
    /// Primary application icon for bundles, taskbars, and app switchers.
    App,
    /// Tray/status icon.
    Tray,
    /// Document/file association icon.
    Document,
    /// Installer/package icon.
    Installer,
}

/// Icon file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppIconFormat {
    /// PNG bitmap icon.
    Png,
    /// Windows `.ico` icon.
    Ico,
    /// macOS `.icns` icon.
    Icns,
    /// SVG vector icon.
    Svg,
}

impl AppIconFormat {
    /// Infer an icon format from a path extension.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        match path
            .as_ref()
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png") => Some(Self::Png),
            Some("ico") => Some(Self::Ico),
            Some("icns") => Some(Self::Icns),
            Some("svg") => Some(Self::Svg),
            _ => None,
        }
    }

    /// Common file extension for this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Ico => "ico",
            Self::Icns => "icns",
            Self::Svg => "svg",
        }
    }

    /// MIME type for this icon format.
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Ico => "image/x-icon",
            Self::Icns => "image/icns",
            Self::Svg => "image/svg+xml",
        }
    }
}

/// One checked icon asset declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIconAsset {
    purpose: AppIconPurpose,
    path: PathBuf,
    format: AppIconFormat,
    size_px: Option<u32>,
    template: bool,
}

impl AppIconAsset {
    /// Semantic icon purpose.
    pub fn purpose(&self) -> AppIconPurpose {
        self.purpose
    }

    /// Icon path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Icon format.
    pub fn format(&self) -> AppIconFormat {
        self.format
    }

    /// Square bitmap size in physical pixels when known.
    pub fn size_px(&self) -> Option<u32> {
        self.size_px
    }

    /// Whether this is a monochrome/template icon.
    pub fn is_template(&self) -> bool {
        self.template
    }
}

/// Builder for one checked icon asset declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIconAssetBuilder {
    purpose: AppIconPurpose,
    path: PathBuf,
    format: Option<AppIconFormat>,
    size_px: Option<u32>,
    template: bool,
    require_existing_path: bool,
}

impl AppIconAssetBuilder {
    /// Create an app icon asset declaration.
    pub fn app(path: impl Into<PathBuf>) -> Self {
        Self::new(AppIconPurpose::App, path)
    }

    /// Create a tray icon asset declaration.
    pub fn tray(path: impl Into<PathBuf>) -> Self {
        Self::new(AppIconPurpose::Tray, path)
    }

    /// Create a document icon asset declaration.
    pub fn document(path: impl Into<PathBuf>) -> Self {
        Self::new(AppIconPurpose::Document, path)
    }

    /// Create an installer icon asset declaration.
    pub fn installer(path: impl Into<PathBuf>) -> Self {
        Self::new(AppIconPurpose::Installer, path)
    }

    /// Create an icon asset declaration for a purpose.
    pub fn new(purpose: AppIconPurpose, path: impl Into<PathBuf>) -> Self {
        Self {
            purpose,
            path: path.into(),
            format: None,
            size_px: None,
            template: false,
            require_existing_path: false,
        }
    }

    /// Override the inferred icon format.
    pub fn format(mut self, format: AppIconFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Set square bitmap size in physical pixels.
    pub fn size_px(mut self, size_px: u32) -> Self {
        self.size_px = Some(size_px);
        self
    }

    /// Mark this as a monochrome/template icon.
    pub fn template(mut self) -> Self {
        self.template = true;
        self
    }

    /// Require the path to exist during validation.
    pub fn require_existing_path(mut self) -> Self {
        self.require_existing_path = true;
        self
    }

    /// Validate this icon asset declaration.
    pub fn validate(&self) -> Result<()> {
        validate_non_empty_path(&self.path, "app icon path")?;
        if self.require_existing_path {
            anyhow::ensure!(
                self.path.exists(),
                "app icon path does not exist: {}",
                self.path.display()
            );
            anyhow::ensure!(
                self.path.is_file(),
                "app icon path must be a file: {}",
                self.path.display()
            );
        }
        let format = self
            .format
            .or_else(|| AppIconFormat::from_path(&self.path))
            .ok_or_else(|| anyhow!("app icon format must be one of png, ico, icns, or svg"))?;
        if let Some(size_px) = self.size_px {
            anyhow::ensure!(size_px > 0, "app icon size must be greater than zero");
            anyhow::ensure!(size_px <= 4096, "app icon size must be at most 4096px");
            anyhow::ensure!(
                !matches!(format, AppIconFormat::Svg),
                "SVG app icons should not declare a fixed pixel size"
            );
        }
        Ok(())
    }

    /// Build a checked icon asset declaration.
    pub fn build_checked(self) -> Result<AppIconAsset> {
        self.validate()?;
        Ok(AppIconAsset {
            purpose: self.purpose,
            format: self
                .format
                .or_else(|| AppIconFormat::from_path(&self.path))
                .expect("icon format validated"),
            path: self.path,
            size_px: self.size_px,
            template: self.template,
        })
    }
}

/// Checked app icon asset set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIconSet {
    icons: Vec<AppIconAsset>,
}

impl AppIconSet {
    /// Checked icon declarations.
    pub fn icons(&self) -> &[AppIconAsset] {
        &self.icons
    }

    /// Icons for one semantic purpose.
    pub fn icons_for(&self, purpose: AppIconPurpose) -> Vec<&AppIconAsset> {
        self.icons
            .iter()
            .filter(|icon| icon.purpose == purpose)
            .collect()
    }
}

/// Builder for grouped app icon assets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppIconSetBuilder {
    icons: Vec<AppIconAssetBuilder>,
}

impl AppIconSetBuilder {
    /// Create an empty icon set builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one icon declaration.
    pub fn icon(mut self, icon: AppIconAssetBuilder) -> Self {
        self.icons.push(icon);
        self
    }

    /// Add multiple icon declarations.
    pub fn icons(mut self, icons: impl IntoIterator<Item = AppIconAssetBuilder>) -> Self {
        self.icons.extend(icons);
        self
    }

    /// Return configured icon builders.
    pub fn configured_icons(&self) -> &[AppIconAssetBuilder] {
        &self.icons
    }

    /// Validate every icon declaration and reject exact duplicate entries.
    pub fn validate(&self) -> Result<()> {
        self.build_icons().map(|_| ())
    }

    /// Build a checked icon set.
    pub fn build_checked(self) -> Result<AppIconSet> {
        Ok(AppIconSet {
            icons: self.build_icons()?,
        })
    }

    fn build_icons(&self) -> Result<Vec<AppIconAsset>> {
        anyhow::ensure!(
            !self.icons.is_empty(),
            "app icon set must include at least one icon"
        );
        let mut icons = Vec::with_capacity(self.icons.len());
        for icon in &self.icons {
            let icon = icon.clone().build_checked()?;
            anyhow::ensure!(
                !icons.iter().any(|existing: &AppIconAsset| {
                    existing.purpose == icon.purpose
                        && existing.format == icon.format
                        && existing.size_px == icon.size_px
                        && existing.path == icon.path
                }),
                "app icon asset declared more than once: {}",
                icon.path.display()
            );
            icons.push(icon);
        }
        Ok(icons)
    }
}

/// Requested native file icon size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileIconSize {
    /// Small list/tree icon, usually 16px.
    Small,
    /// Normal file browser icon, usually 32px.
    Normal,
    /// Large preview icon, usually 64px.
    Large,
    /// Explicit square icon size in physical pixels.
    Custom(u32),
}

impl FileIconSize {
    /// Size in physical pixels requested from the platform backend.
    pub fn pixels(self) -> u32 {
        match self {
            Self::Small => 16,
            Self::Normal => 32,
            Self::Large => 64,
            Self::Custom(size) => size,
        }
    }

    /// Stable key for diagnostics and generated icon policies.
    pub fn key(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Normal => "normal",
            Self::Large => "large",
            Self::Custom(_) => "custom",
        }
    }

    fn validate(self) -> Result<()> {
        let size = self.pixels();
        anyhow::ensure!(size > 0, "file icon size must be greater than zero");
        anyhow::ensure!(size <= 1024, "file icon size cannot exceed 1024px");
        Ok(())
    }
}

/// Checked request for a native icon representing a file, folder, or planned path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIconRequest {
    path: PathBuf,
    size: FileIconSize,
    require_existing_path: bool,
    allow_generic_fallback: bool,
}

impl FileIconRequest {
    /// Path whose native icon should be requested.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Requested icon size.
    pub fn size(&self) -> FileIconSize {
        self.size
    }

    /// Whether validation required the path to exist.
    pub fn requires_existing_path(&self) -> bool {
        self.require_existing_path
    }

    /// Whether platform code may return a generic extension/folder icon if no concrete icon exists.
    pub fn allows_generic_fallback(&self) -> bool {
        self.allow_generic_fallback
    }

    /// Normalized extension hint for generic icon fallback.
    pub fn extension_hint(&self) -> Option<String> {
        self.path.extension().and_then(|extension| {
            let extension = normalize_drop_extension(&extension.to_string_lossy());
            (!extension.is_empty()).then_some(extension)
        })
    }
}

/// Builder for checked native file icon requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIconRequestBuilder {
    path: PathBuf,
    size: FileIconSize,
    require_existing_path: bool,
    canonicalize_path: bool,
    allow_generic_fallback: bool,
}

impl FileIconRequestBuilder {
    /// Request a native icon for a file, folder, or planned path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            size: FileIconSize::Normal,
            require_existing_path: false,
            canonicalize_path: false,
            allow_generic_fallback: true,
        }
    }

    /// Use Electron-style small icon size.
    pub fn small(mut self) -> Self {
        self.size = FileIconSize::Small;
        self
    }

    /// Use Electron-style normal icon size.
    pub fn normal(mut self) -> Self {
        self.size = FileIconSize::Normal;
        self
    }

    /// Use Electron-style large icon size.
    pub fn large(mut self) -> Self {
        self.size = FileIconSize::Large;
        self
    }

    /// Request a custom square icon size in physical pixels.
    pub fn custom_size_px(mut self, size: u32) -> Self {
        self.size = FileIconSize::Custom(size);
        self
    }

    /// Require the target path to exist.
    pub fn require_existing_path(mut self) -> Self {
        self.require_existing_path = true;
        self
    }

    /// Canonicalize the target path before building.
    pub fn canonicalize_path(mut self) -> Self {
        self.canonicalize_path = true;
        self.require_existing_path = true;
        self
    }

    /// Set whether generic extension/folder fallback icons are acceptable.
    pub fn allow_generic_fallback(mut self, allow: bool) -> Self {
        self.allow_generic_fallback = allow;
        self
    }

    /// Validate this native file icon request.
    pub fn validate(&self) -> Result<()> {
        validate_non_empty_path(&self.path, "file icon path")?;
        self.size.validate()?;
        if self.require_existing_path {
            anyhow::ensure!(
                self.path.exists(),
                "file icon path does not exist: {}",
                self.path.display()
            );
        }
        if !self.path.exists() {
            anyhow::ensure!(
                self.allow_generic_fallback,
                "file icon request for a missing path must allow generic fallback: {}",
                self.path.display()
            );
            anyhow::ensure!(
                self.path.extension().is_some(),
                "file icon request for a missing path must include an extension hint: {}",
                self.path.display()
            );
        }
        Ok(())
    }

    /// Build a checked native file icon request.
    pub fn build_checked(mut self) -> Result<FileIconRequest> {
        self.validate()?;
        if self.canonicalize_path {
            self.path = self.path.canonicalize().map_err(|error| {
                anyhow!(
                    "could not canonicalize file icon path {}: {error}",
                    self.path.display()
                )
            })?;
        }
        Ok(FileIconRequest {
            path: self.path,
            size: self.size,
            require_existing_path: self.require_existing_path,
            allow_generic_fallback: self.allow_generic_fallback,
        })
    }
}

/// Privacy-sensitive capability declared for packaging metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppPrivacyPermissionKind {
    /// Camera/video capture.
    Camera,
    /// Microphone/audio input.
    Microphone,
    /// Screen/window capture.
    ScreenCapture,
    /// Location/geolocation.
    Location,
    /// Native notifications.
    Notifications,
    /// Filesystem access beyond transient user-selected files.
    Filesystem,
    /// Network access.
    Network,
    /// USB device access.
    UsbDevices,
    /// HID device access.
    HidDevices,
    /// Serial port access.
    SerialPorts,
    /// Bluetooth device/service access.
    Bluetooth,
}

impl AppPrivacyPermissionKind {
    /// Stable key used in package manifests and diagnostics.
    pub fn key(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::ScreenCapture => "screen-capture",
            Self::Location => "location",
            Self::Notifications => "notifications",
            Self::Filesystem => "filesystem",
            Self::Network => "network",
            Self::UsbDevices => "usb-devices",
            Self::HidDevices => "hid-devices",
            Self::SerialPorts => "serial-ports",
            Self::Bluetooth => "bluetooth",
        }
    }

    /// macOS Info.plist usage-description key when one is commonly required.
    pub fn macos_usage_description_key(self) -> Option<&'static str> {
        match self {
            Self::Camera => Some("NSCameraUsageDescription"),
            Self::Microphone => Some("NSMicrophoneUsageDescription"),
            Self::Location => Some("NSLocationWhenInUseUsageDescription"),
            Self::Bluetooth => Some("NSBluetoothAlwaysUsageDescription"),
            Self::ScreenCapture
            | Self::Notifications
            | Self::Filesystem
            | Self::Network
            | Self::UsbDevices
            | Self::HidDevices
            | Self::SerialPorts => None,
        }
    }
}

/// One checked privacy permission declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPrivacyPermission {
    kind: AppPrivacyPermissionKind,
    reason: String,
}

impl AppPrivacyPermission {
    /// Permission kind.
    pub fn kind(&self) -> AppPrivacyPermissionKind {
        self.kind
    }

    /// User-facing reason/usage description.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// macOS usage-description entry when this permission maps to one.
    pub fn macos_usage_description(&self) -> Option<MacOsUsageDescription> {
        Some(MacOsUsageDescription {
            key: self.kind.macos_usage_description_key()?.to_string(),
            value: self.reason.clone(),
        })
    }
}

/// Builder for one privacy permission declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPrivacyPermissionBuilder {
    kind: AppPrivacyPermissionKind,
    reason: String,
}

impl AppPrivacyPermissionBuilder {
    /// Declare camera usage.
    pub fn camera(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Camera, reason)
    }

    /// Declare microphone usage.
    pub fn microphone(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Microphone, reason)
    }

    /// Declare screen capture usage.
    pub fn screen_capture(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::ScreenCapture, reason)
    }

    /// Declare location usage.
    pub fn location(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Location, reason)
    }

    /// Declare notification usage.
    pub fn notifications(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Notifications, reason)
    }

    /// Declare filesystem usage.
    pub fn filesystem(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Filesystem, reason)
    }

    /// Declare network usage.
    pub fn network(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Network, reason)
    }

    /// Declare USB device usage.
    pub fn usb_devices(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::UsbDevices, reason)
    }

    /// Declare HID device usage.
    pub fn hid_devices(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::HidDevices, reason)
    }

    /// Declare serial port usage.
    pub fn serial_ports(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::SerialPorts, reason)
    }

    /// Declare Bluetooth usage.
    pub fn bluetooth(reason: impl Into<String>) -> Self {
        Self::new(AppPrivacyPermissionKind::Bluetooth, reason)
    }

    /// Create a declaration for a privacy-sensitive permission kind.
    pub fn new(kind: AppPrivacyPermissionKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }

    /// Create a packaging declaration from a runtime capability.
    pub fn from_capability(capability: &Capability, reason: impl Into<String>) -> Option<Self> {
        let kind = match capability {
            Capability::Camera => AppPrivacyPermissionKind::Camera,
            Capability::Microphone => AppPrivacyPermissionKind::Microphone,
            Capability::ScreenCapture => AppPrivacyPermissionKind::ScreenCapture,
            Capability::Location => AppPrivacyPermissionKind::Location,
            Capability::UsbDevice => AppPrivacyPermissionKind::UsbDevices,
            Capability::HidDevice => AppPrivacyPermissionKind::HidDevices,
            Capability::SerialPort => AppPrivacyPermissionKind::SerialPorts,
            Capability::Bluetooth => AppPrivacyPermissionKind::Bluetooth,
            Capability::Notification => AppPrivacyPermissionKind::Notifications,
            Capability::FilesystemRead { .. } | Capability::FilesystemWrite { .. } => {
                AppPrivacyPermissionKind::Filesystem
            }
            Capability::Network { .. } => AppPrivacyPermissionKind::Network,
            Capability::OpenExternalUrl
            | Capability::ShellExecute
            | Capability::ClipboardRead
            | Capability::ClipboardWrite => return None,
        };
        Some(Self::new(kind, reason))
    }

    /// Validate this privacy declaration.
    pub fn validate(&self) -> Result<()> {
        validate_app_metadata_text(&self.reason, "privacy permission reason", 256, false)
    }

    /// Build a checked privacy declaration.
    pub fn build_checked(self) -> Result<AppPrivacyPermission> {
        self.validate()?;
        Ok(AppPrivacyPermission {
            kind: self.kind,
            reason: self.reason,
        })
    }
}

/// Checked privacy permission manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPrivacyManifest {
    permissions: Vec<AppPrivacyPermission>,
}

impl AppPrivacyManifest {
    /// Checked permission declarations.
    pub fn permissions(&self) -> &[AppPrivacyPermission] {
        &self.permissions
    }

    /// Whether a permission kind is declared.
    pub fn declares(&self, kind: AppPrivacyPermissionKind) -> bool {
        self.permissions
            .iter()
            .any(|permission| permission.kind == kind)
    }

    /// macOS Info.plist usage descriptions derived from this manifest.
    pub fn macos_usage_descriptions(&self) -> Vec<MacOsUsageDescription> {
        self.permissions
            .iter()
            .filter_map(AppPrivacyPermission::macos_usage_description)
            .collect()
    }
}

/// Builder for grouped privacy permission declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPrivacyManifestBuilder {
    permissions: Vec<AppPrivacyPermissionBuilder>,
}

impl AppPrivacyManifestBuilder {
    /// Create an empty privacy manifest builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one permission declaration.
    pub fn permission(mut self, permission: AppPrivacyPermissionBuilder) -> Self {
        self.permissions.push(permission);
        self
    }

    /// Add multiple permission declarations.
    pub fn permissions(
        mut self,
        permissions: impl IntoIterator<Item = AppPrivacyPermissionBuilder>,
    ) -> Self {
        self.permissions.extend(permissions);
        self
    }

    /// Return configured permission builders.
    pub fn configured_permissions(&self) -> &[AppPrivacyPermissionBuilder] {
        &self.permissions
    }

    /// Validate every declaration and reject duplicate permission kinds.
    pub fn validate(&self) -> Result<()> {
        self.build_permissions().map(|_| ())
    }

    /// Build a checked privacy manifest.
    pub fn build_checked(self) -> Result<AppPrivacyManifest> {
        Ok(AppPrivacyManifest {
            permissions: self.build_permissions()?,
        })
    }

    fn build_permissions(&self) -> Result<Vec<AppPrivacyPermission>> {
        anyhow::ensure!(
            !self.permissions.is_empty(),
            "privacy manifest must include at least one permission"
        );
        let mut permissions = Vec::with_capacity(self.permissions.len());
        for permission in &self.permissions {
            let permission = permission.clone().build_checked()?;
            anyhow::ensure!(
                !permissions
                    .iter()
                    .any(|existing: &AppPrivacyPermission| existing.kind == permission.kind),
                "privacy permission declared more than once: {}",
                permission.kind.key()
            );
            permissions.push(permission);
        }
        Ok(permissions)
    }
}

/// macOS Info.plist usage-description entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacOsUsageDescription {
    key: String,
    value: String,
}

impl MacOsUsageDescription {
    /// Info.plist key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// User-facing usage description.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// macOS URL scheme metadata shaped like one `CFBundleURLTypes` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacOsUrlTypeDeclaration {
    name: String,
    schemes: Vec<String>,
}

impl MacOsUrlTypeDeclaration {
    /// Bundle URL name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// URL schemes claimed by this entry.
    pub fn schemes(&self) -> &[String] {
        &self.schemes
    }
}

/// macOS document metadata shaped like one `CFBundleDocumentTypes` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacOsDocumentTypeDeclaration {
    name: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
    role: FileAssociationRole,
}

impl MacOsDocumentTypeDeclaration {
    /// Document type name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Declared file extensions.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Declared MIME types.
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    /// Bundle document role.
    pub fn role(&self) -> FileAssociationRole {
        self.role
    }
}

/// Windows file association metadata shaped for installer/registry generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsFileAssociationDeclaration {
    name: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
    prog_id: String,
    role: FileAssociationRole,
    description: Option<String>,
}

impl WindowsFileAssociationDeclaration {
    /// User-facing association name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Declared file extensions.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Declared MIME types.
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }

    /// Stable Windows ProgID.
    pub fn prog_id(&self) -> &str {
        &self.prog_id
    }

    /// Whether the app views or edits this type.
    pub fn role(&self) -> FileAssociationRole {
        self.role
    }

    /// Optional description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// Semantic purpose for a native file drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDropPurpose {
    /// Open one or more documents.
    OpenDocument,
    /// Import files into the current workspace or document.
    ImportFiles,
    /// Import or attach folders.
    ImportFolder,
    /// Open media sources such as audio, video, or images.
    MediaSource,
    /// Open a project or workspace root.
    ProjectWorkspace,
    /// App-defined drop purpose.
    Custom(String),
}

impl FileDropPurpose {
    /// Create a custom drop purpose label.
    pub fn custom(purpose: impl Into<String>) -> Self {
        Self::Custom(purpose.into())
    }

    /// User-facing purpose label.
    pub fn label(&self) -> &str {
        match self {
            Self::OpenDocument => "open-document",
            Self::ImportFiles => "import-files",
            Self::ImportFolder => "import-folder",
            Self::MediaSource => "media-source",
            Self::ProjectWorkspace => "project-workspace",
            Self::Custom(purpose) => purpose,
        }
    }

    /// Validate custom purpose labels.
    pub fn validate(&self) -> Result<()> {
        if let Self::Custom(purpose) = self {
            validate_app_metadata_text(purpose, "file drop purpose", 64, false)?;
        }
        Ok(())
    }
}

/// Path kind accepted by a file-drop intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDropPathKind {
    /// Accept files or directories.
    Any,
    /// Accept only files.
    FilesOnly,
    /// Accept only directories.
    DirectoriesOnly,
}

/// Checked file-drop intent for imports, project opens, and media drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDropIntent {
    purpose: FileDropPurpose,
    paths: Vec<PathBuf>,
}

impl FileDropIntent {
    /// Semantic purpose for this drop.
    pub fn purpose(&self) -> &FileDropPurpose {
        &self.purpose
    }

    /// Accepted paths after validation, canonicalization, and deduplication.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Return the first accepted path.
    pub fn first_path(&self) -> Option<&Path> {
        self.paths.first().map(PathBuf::as_path)
    }

    /// Consume this intent into accepted paths.
    pub fn into_paths(self) -> Vec<PathBuf> {
        self.paths
    }
}

/// Builder for validating native file drops before app-owned import/open work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDropIntentBuilder {
    purpose: FileDropPurpose,
    paths: Vec<PathBuf>,
    path_kind: FileDropPathKind,
    allowed_extensions: Vec<String>,
    max_paths: Option<usize>,
    require_existing_paths: bool,
    canonicalize_paths: bool,
}

impl FileDropIntentBuilder {
    /// Create an empty file-drop intent builder.
    pub fn new(purpose: FileDropPurpose) -> Self {
        Self {
            purpose,
            paths: Vec::new(),
            path_kind: FileDropPathKind::Any,
            allowed_extensions: Vec::new(),
            max_paths: None,
            require_existing_paths: true,
            canonicalize_paths: false,
        }
    }

    /// Create a document-open file drop.
    pub fn open_document() -> Self {
        Self::new(FileDropPurpose::OpenDocument).files_only()
    }

    /// Create a file-import drop.
    pub fn import_files() -> Self {
        Self::new(FileDropPurpose::ImportFiles).files_only()
    }

    /// Create a folder-import drop.
    pub fn import_folder() -> Self {
        Self::new(FileDropPurpose::ImportFolder).directories_only()
    }

    /// Create a media-source drop with common media extensions.
    pub fn media_source() -> Self {
        Self::new(FileDropPurpose::MediaSource)
            .files_only()
            .extensions([
                "aac", "aiff", "avif", "bmp", "flac", "gif", "heic", "heif", "jpg", "jpeg", "m4a",
                "m4v", "mkv", "mov", "mp3", "mp4", "mpeg", "mpg", "ogg", "ogv", "opus", "png",
                "wav", "webm", "webp",
            ])
    }

    /// Create a project/workspace drop.
    pub fn project_workspace() -> Self {
        Self::new(FileDropPurpose::ProjectWorkspace)
    }

    /// Add one dropped path.
    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.paths.push(path.into());
        self
    }

    /// Add multiple dropped paths.
    pub fn paths(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.paths.extend(paths.into_iter().map(Into::into));
        self
    }

    /// Accept only files.
    pub fn files_only(mut self) -> Self {
        self.path_kind = FileDropPathKind::FilesOnly;
        self
    }

    /// Accept only directories.
    pub fn directories_only(mut self) -> Self {
        self.path_kind = FileDropPathKind::DirectoriesOnly;
        self
    }

    /// Accept files and directories.
    pub fn any_path_kind(mut self) -> Self {
        self.path_kind = FileDropPathKind::Any;
        self
    }

    /// Accept only paths with one of the provided extensions.
    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.allowed_extensions = extensions
            .into_iter()
            .map(|extension| normalize_drop_extension(extension.as_ref()))
            .filter(|extension| !extension.is_empty())
            .collect();
        self.allowed_extensions.sort();
        self.allowed_extensions.dedup();
        self
    }

    /// Limit the number of accepted paths.
    pub fn max_paths(mut self, max_paths: usize) -> Self {
        self.max_paths = Some(max_paths);
        self
    }

    /// Require all dropped paths to exist. This is the default.
    pub fn require_existing_paths(mut self) -> Self {
        self.require_existing_paths = true;
        self
    }

    /// Allow missing paths for virtual/test drops.
    pub fn allow_missing_paths(mut self) -> Self {
        self.require_existing_paths = false;
        self
    }

    /// Canonicalize accepted paths. This also requires existing paths.
    pub fn canonicalize_paths(mut self) -> Self {
        self.canonicalize_paths = true;
        self.require_existing_paths = true;
        self
    }

    /// Preserve accepted paths exactly as configured.
    pub fn preserve_paths(mut self) -> Self {
        self.canonicalize_paths = false;
        self
    }

    /// Return the configured paths.
    pub fn configured_paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Return the configured path-kind policy.
    pub fn path_kind(&self) -> FileDropPathKind {
        self.path_kind
    }

    /// Return allowed extensions.
    pub fn allowed_extensions(&self) -> &[String] {
        &self.allowed_extensions
    }

    /// Validate this file-drop intent.
    pub fn validate(&self) -> Result<()> {
        self.resolve_paths().map(|_| ())
    }

    /// Build the checked file-drop intent.
    pub fn build_checked(self) -> Result<FileDropIntent> {
        let paths = self.resolve_paths()?;
        Ok(FileDropIntent {
            purpose: self.purpose,
            paths,
        })
    }

    fn resolve_paths(&self) -> Result<Vec<PathBuf>> {
        self.purpose.validate()?;
        anyhow::ensure!(
            !self.paths.is_empty(),
            "file drop must include at least one path"
        );
        if let Some(max_paths) = self.max_paths {
            anyhow::ensure!(
                max_paths > 0,
                "file drop max paths must be greater than zero"
            );
            anyhow::ensure!(
                self.paths.len() <= max_paths,
                "file drop included {} paths, exceeding max {max_paths}",
                self.paths.len()
            );
        }

        let mut resolved = Vec::new();
        for path in &self.paths {
            anyhow::ensure!(
                !path.as_os_str().is_empty(),
                "file drop path cannot be empty"
            );
            validate_no_nul(&path.to_string_lossy(), "file drop path")?;

            if self.require_existing_paths {
                anyhow::ensure!(
                    path.exists(),
                    "file drop path must exist: {}",
                    path.display()
                );
            }

            if self.require_existing_paths || path.exists() {
                match self.path_kind {
                    FileDropPathKind::Any => {}
                    FileDropPathKind::FilesOnly => {
                        anyhow::ensure!(
                            path.is_file(),
                            "file drop path must be a file: {}",
                            path.display()
                        );
                    }
                    FileDropPathKind::DirectoriesOnly => {
                        anyhow::ensure!(
                            path.is_dir(),
                            "file drop path must be a directory: {}",
                            path.display()
                        );
                    }
                }
            }

            if !self.allowed_extensions.is_empty() {
                let extension = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(normalize_drop_extension);
                anyhow::ensure!(
                    extension
                        .as_ref()
                        .is_some_and(|extension| self.allowed_extensions.contains(extension)),
                    "file drop path extension is not accepted: {}",
                    path.display()
                );
            }

            let path = if self.canonicalize_paths {
                fs::canonicalize(path).with_context(|| {
                    format!("failed to canonicalize file drop {}", path.display())
                })?
            } else {
                path.clone()
            };

            if !resolved.iter().any(|existing| existing == &path) {
                resolved.push(path);
            }
        }

        Ok(resolved)
    }
}

/// Data source for an app-owned outbound file drag/export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileExportDragItem {
    /// Drag an existing file path out to another app or the desktop.
    ExistingPath(PathBuf),
    /// Drag generated bytes as a promised/virtual file.
    VirtualFile {
        /// Suggested file name.
        file_name: String,
        /// Optional MIME type.
        mime_type: Option<String>,
        /// Generated file bytes.
        bytes: Vec<u8>,
    },
}

impl FileExportDragItem {
    /// Create an existing-path export item.
    pub fn existing_path(path: impl Into<PathBuf>) -> Self {
        Self::ExistingPath(path.into())
    }

    /// Create a generated/virtual file export item.
    pub fn virtual_file(file_name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self::VirtualFile {
            file_name: file_name.into(),
            mime_type: None,
            bytes: bytes.into(),
        }
    }

    /// Add or replace the MIME type for a virtual file.
    pub fn mime_type(mut self, mime_type: impl Into<String>) -> Self {
        if let Self::VirtualFile {
            mime_type: current, ..
        } = &mut self
        {
            *current = Some(mime_type.into());
        }
        self
    }

    /// Suggested display name for the drag item.
    pub fn display_name(&self) -> &str {
        match self {
            Self::ExistingPath(path) => path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
            Self::VirtualFile { file_name, .. } => file_name,
        }
    }

    /// Optional MIME type.
    pub fn mime_type_hint(&self) -> Option<&str> {
        match self {
            Self::ExistingPath(_) => None,
            Self::VirtualFile { mime_type, .. } => mime_type.as_deref(),
        }
    }
}

/// Checked outbound file drag/export descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExportDragIntent {
    purpose: String,
    items: Vec<FileExportDragItem>,
    require_existing_paths: bool,
    max_virtual_file_bytes: usize,
}

impl FileExportDragIntent {
    /// User-facing purpose for the export drag.
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// Items to expose to the platform drag session.
    pub fn items(&self) -> &[FileExportDragItem] {
        &self.items
    }

    /// Whether existing-path items were required to exist during validation.
    pub fn requires_existing_paths(&self) -> bool {
        self.require_existing_paths
    }

    /// Maximum allowed virtual file size in bytes.
    pub fn max_virtual_file_bytes(&self) -> usize {
        self.max_virtual_file_bytes
    }

    /// Runtime capabilities required to perform this export.
    pub fn required_capabilities(&self) -> Vec<Capability> {
        if self
            .items
            .iter()
            .any(|item| matches!(item, FileExportDragItem::ExistingPath(_)))
        {
            vec![Capability::FilesystemRead {
                scope: PathScope::UserSelected,
            }]
        } else {
            Vec::new()
        }
    }

    /// Returns true when the export contains generated/virtual file bytes.
    pub fn has_virtual_files(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item, FileExportDragItem::VirtualFile { .. }))
    }
}

/// Builder for checked outbound file drags and file promises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileExportDragIntentBuilder {
    purpose: String,
    items: Vec<FileExportDragItem>,
    require_existing_paths: bool,
    max_items: Option<usize>,
    max_virtual_file_bytes: usize,
}

impl FileExportDragIntentBuilder {
    /// Create an empty outbound file drag builder.
    pub fn new(purpose: impl Into<String>) -> Self {
        Self {
            purpose: purpose.into(),
            items: Vec::new(),
            require_existing_paths: true,
            max_items: None,
            max_virtual_file_bytes: 256 * 1024 * 1024,
        }
    }

    /// Create a builder for exporting existing files.
    pub fn existing_files(purpose: impl Into<String>) -> Self {
        Self::new(purpose)
    }

    /// Create a builder for exporting generated files.
    pub fn generated_files(purpose: impl Into<String>) -> Self {
        Self::new(purpose).allow_missing_paths()
    }

    /// Add an existing file path.
    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.items.push(FileExportDragItem::existing_path(path));
        self
    }

    /// Add multiple existing file paths.
    pub fn paths(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.items
            .extend(paths.into_iter().map(FileExportDragItem::existing_path));
        self
    }

    /// Add a generated/virtual file.
    pub fn virtual_file(mut self, file_name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.items
            .push(FileExportDragItem::virtual_file(file_name, bytes));
        self
    }

    /// Add a generated/virtual file with a MIME type.
    pub fn virtual_file_with_mime(
        mut self,
        file_name: impl Into<String>,
        mime_type: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        self.items
            .push(FileExportDragItem::virtual_file(file_name, bytes).mime_type(mime_type));
        self
    }

    /// Add an already constructed export item.
    pub fn item(mut self, item: FileExportDragItem) -> Self {
        self.items.push(item);
        self
    }

    /// Require existing-path items to exist. This is the default.
    pub fn require_existing_paths(mut self) -> Self {
        self.require_existing_paths = true;
        self
    }

    /// Allow existing-path items that do not exist yet, for test or backend-created files.
    pub fn allow_missing_paths(mut self) -> Self {
        self.require_existing_paths = false;
        self
    }

    /// Limit the number of drag items.
    pub fn max_items(mut self, max_items: usize) -> Self {
        self.max_items = Some(max_items);
        self
    }

    /// Limit the size of each virtual file.
    pub fn max_virtual_file_bytes(mut self, max_virtual_file_bytes: usize) -> Self {
        self.max_virtual_file_bytes = max_virtual_file_bytes;
        self
    }

    /// Return configured items.
    pub fn configured_items(&self) -> &[FileExportDragItem] {
        &self.items
    }

    /// Validate this export drag intent.
    pub fn validate(&self) -> Result<()> {
        self.resolve_items().map(|_| ())
    }

    /// Build the checked export drag intent.
    pub fn build_checked(self) -> Result<FileExportDragIntent> {
        let items = self.resolve_items()?;
        Ok(FileExportDragIntent {
            purpose: self.purpose,
            items,
            require_existing_paths: self.require_existing_paths,
            max_virtual_file_bytes: self.max_virtual_file_bytes,
        })
    }

    fn resolve_items(&self) -> Result<Vec<FileExportDragItem>> {
        validate_app_metadata_text(&self.purpose, "file export drag purpose", 128, false)?;
        anyhow::ensure!(
            !self.items.is_empty(),
            "file export drag must include at least one item"
        );
        if let Some(max_items) = self.max_items {
            anyhow::ensure!(
                max_items > 0,
                "file export drag max items must be greater than zero"
            );
            anyhow::ensure!(
                self.items.len() <= max_items,
                "file export drag included {} items, exceeding max {max_items}",
                self.items.len()
            );
        }
        anyhow::ensure!(
            self.max_virtual_file_bytes > 0,
            "file export drag virtual file byte limit must be greater than zero"
        );

        let mut resolved = Vec::new();
        for item in &self.items {
            match item {
                FileExportDragItem::ExistingPath(path) => {
                    validate_export_drag_path(path, self.require_existing_paths)?;
                    if !resolved.iter().any(|existing| existing == item) {
                        resolved.push(item.clone());
                    }
                }
                FileExportDragItem::VirtualFile {
                    file_name,
                    mime_type,
                    bytes,
                } => {
                    validate_export_file_name(file_name)?;
                    if let Some(mime_type) = mime_type {
                        validate_export_mime_type(mime_type)?;
                    }
                    anyhow::ensure!(
                        !bytes.is_empty(),
                        "file export drag virtual file bytes cannot be empty"
                    );
                    anyhow::ensure!(
                        bytes.len() <= self.max_virtual_file_bytes,
                        "file export drag virtual file {} exceeds {} bytes",
                        file_name,
                        self.max_virtual_file_bytes
                    );
                    resolved.push(item.clone());
                }
            }
        }

        Ok(resolved)
    }
}

/// Coarse content kind for app-owned file intake routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIntakeKind {
    /// A directory or folder.
    Directory,
    /// A project/workspace file.
    Project,
    /// An image file.
    Image,
    /// An audio file.
    Audio,
    /// A video file.
    Video,
    /// A PDF document.
    Pdf,
    /// A text or markup document.
    Text,
    /// Structured data such as JSON, CSV, TOML, or YAML.
    Data,
    /// Archive/package file.
    Archive,
    /// Unknown or unsupported extension.
    Unknown,
}

impl FileIntakeKind {
    /// Whether this kind is visual/audio/video media.
    pub fn is_media(self) -> bool {
        matches!(self, Self::Image | Self::Audio | Self::Video)
    }

    /// Whether this kind should normally open in an editor/document view.
    pub fn is_document(self) -> bool {
        matches!(self, Self::Pdf | Self::Text | Self::Data)
    }
}

/// One classified file-intake path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIntakeEntry {
    path: PathBuf,
    kind: FileIntakeKind,
    extension: Option<String>,
}

impl FileIntakeEntry {
    /// Path after validation and optional canonicalization.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Classified content kind.
    pub fn kind(&self) -> FileIntakeKind {
        self.kind
    }

    /// Normalized lowercase extension, when present.
    pub fn extension(&self) -> Option<&str> {
        self.extension.as_deref()
    }
}

/// Checked file-intake classification result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIntakePlan {
    entries: Vec<FileIntakeEntry>,
}

impl FileIntakePlan {
    /// Classified entries in input order after deduplication.
    pub fn entries(&self) -> &[FileIntakeEntry] {
        &self.entries
    }

    /// Consume this plan into entries.
    pub fn into_entries(self) -> Vec<FileIntakeEntry> {
        self.entries
    }

    /// Return paths matching a kind.
    pub fn paths_of_kind(&self, kind: FileIntakeKind) -> Vec<&Path> {
        self.entries
            .iter()
            .filter_map(|entry| (entry.kind == kind).then_some(entry.path()))
            .collect()
    }

    /// Whether any entry is media.
    pub fn has_media(&self) -> bool {
        self.entries.iter().any(|entry| entry.kind.is_media())
    }

    /// Whether any entry has unknown kind.
    pub fn has_unknown(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.kind == FileIntakeKind::Unknown)
    }
}

/// Builder for classifying app-owned paths from dialogs, drops, recent docs, or deep links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIntakePlanBuilder {
    paths: Vec<PathBuf>,
    max_paths: Option<usize>,
    require_existing_paths: bool,
    canonicalize_paths: bool,
    reject_unknown: bool,
}

impl FileIntakePlanBuilder {
    /// Create an empty file-intake classifier.
    pub fn new() -> Self {
        Self {
            paths: Vec::new(),
            max_paths: None,
            require_existing_paths: true,
            canonicalize_paths: false,
            reject_unknown: false,
        }
    }

    /// Add one path.
    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.paths.push(path.into());
        self
    }

    /// Add multiple paths.
    pub fn paths(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.paths.extend(paths.into_iter().map(Into::into));
        self
    }

    /// Limit the number of paths.
    pub fn max_paths(mut self, max_paths: usize) -> Self {
        self.max_paths = Some(max_paths);
        self
    }

    /// Require all paths to exist. This is the default.
    pub fn require_existing_paths(mut self) -> Self {
        self.require_existing_paths = true;
        self
    }

    /// Allow virtual or not-yet-created paths.
    pub fn allow_missing_paths(mut self) -> Self {
        self.require_existing_paths = false;
        self
    }

    /// Canonicalize paths before classification. This requires existing paths.
    pub fn canonicalize_paths(mut self) -> Self {
        self.canonicalize_paths = true;
        self.require_existing_paths = true;
        self
    }

    /// Reject unknown file kinds instead of carrying them through the plan.
    pub fn reject_unknown(mut self) -> Self {
        self.reject_unknown = true;
        self
    }

    /// Validate configured paths and options.
    pub fn validate(&self) -> Result<()> {
        self.build_entries().map(|_| ())
    }

    /// Build a checked file-intake plan.
    pub fn build_checked(self) -> Result<FileIntakePlan> {
        Ok(FileIntakePlan {
            entries: self.build_entries()?,
        })
    }

    fn build_entries(&self) -> Result<Vec<FileIntakeEntry>> {
        anyhow::ensure!(
            !self.paths.is_empty(),
            "file intake must include at least one path"
        );
        if let Some(max_paths) = self.max_paths {
            anyhow::ensure!(
                max_paths > 0,
                "file intake max paths must be greater than zero"
            );
            anyhow::ensure!(
                self.paths.len() <= max_paths,
                "file intake included {} paths, exceeding max {max_paths}",
                self.paths.len()
            );
        }

        let mut entries = Vec::new();
        for path in &self.paths {
            anyhow::ensure!(
                !path.as_os_str().is_empty(),
                "file intake path cannot be empty"
            );
            validate_no_nul(&path.to_string_lossy(), "file intake path")?;
            if self.require_existing_paths {
                anyhow::ensure!(
                    path.exists(),
                    "file intake path must exist: {}",
                    path.display()
                );
            }

            let path = if self.canonicalize_paths {
                fs::canonicalize(path).with_context(|| {
                    format!("failed to canonicalize file intake {}", path.display())
                })?
            } else {
                path.clone()
            };

            if entries
                .iter()
                .any(|entry: &FileIntakeEntry| entry.path == path)
            {
                continue;
            }

            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(normalize_drop_extension)
                .filter(|extension| !extension.is_empty());
            let kind = classify_file_intake_path(&path, extension.as_deref());
            anyhow::ensure!(
                !self.reject_unknown || kind != FileIntakeKind::Unknown,
                "file intake path kind is unknown: {}",
                path.display()
            );
            entries.push(FileIntakeEntry {
                path,
                kind,
                extension,
            });
        }

        Ok(entries)
    }
}

impl Default for FileIntakePlanBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for adding one or more documents to the OS recent-documents list.
#[derive(Debug, Clone, Default)]
pub struct RecentDocumentsBuilder {
    documents: Vec<PathBuf>,
    require_existing_files: bool,
    canonicalize_paths: bool,
}

impl RecentDocumentsBuilder {
    /// Create an empty recent-documents builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one document path.
    pub fn document(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if !self.documents.iter().any(|existing| existing == &path) {
            self.documents.push(path);
        }
        self
    }

    /// Add multiple document paths.
    pub fn documents(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        for path in paths {
            self = self.document(path);
        }
        self
    }

    /// Require every recent document to currently exist and be a file.
    pub fn require_existing_files(mut self) -> Self {
        self.require_existing_files = true;
        self
    }

    /// Allow missing paths. This is the default and matches lower-level platform APIs.
    pub fn allow_missing(mut self) -> Self {
        self.require_existing_files = false;
        self
    }

    /// Canonicalize paths before adding them to the OS recent-documents list.
    pub fn canonicalize(mut self) -> Self {
        self.canonicalize_paths = true;
        self
    }

    /// Preserve paths exactly as configured. This is the default.
    pub fn preserve_paths(mut self) -> Self {
        self.canonicalize_paths = false;
        self
    }

    /// Returns the configured document paths.
    pub fn configured_documents(&self) -> &[PathBuf] {
        &self.documents
    }

    /// Whether configured documents must exist as files before they are added.
    pub fn requires_existing_files(&self) -> bool {
        self.require_existing_files
    }

    /// Whether paths will be canonicalized before they are added.
    pub fn canonicalizes_paths(&self) -> bool {
        self.canonicalize_paths
    }

    /// Validate that at least one document path was configured.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.documents.is_empty(),
            "at least one recent document path must be configured"
        );
        self.resolve_documents()?;
        Ok(())
    }

    /// Build the validated document list.
    pub fn build(self) -> Result<Vec<PathBuf>> {
        anyhow::ensure!(
            !self.documents.is_empty(),
            "at least one recent document path must be configured"
        );
        self.resolve_documents()
    }

    fn resolve_documents(&self) -> Result<Vec<PathBuf>> {
        let mut documents = Vec::new();
        for path in &self.documents {
            let path = if self.canonicalize_paths {
                fs::canonicalize(path).with_context(|| {
                    format!("failed to canonicalize recent document {}", path.display())
                })?
            } else {
                path.clone()
            };

            if self.require_existing_files {
                anyhow::ensure!(
                    path.is_file(),
                    "recent document must be an existing file: {}",
                    path.display()
                );
            }

            if !documents.iter().any(|existing| existing == &path) {
                documents.push(path);
            }
        }
        Ok(documents)
    }
}

impl From<PathBuf> for RecentDocumentsBuilder {
    fn from(value: PathBuf) -> Self {
        Self::new().document(value)
    }
}

impl<'a> From<&'a Path> for RecentDocumentsBuilder {
    fn from(value: &'a Path) -> Self {
        Self::new().document(value)
    }
}

/// Builder for Windows jump-list tasks and recent workspace entries.
#[derive(Default)]
pub struct JumpListBuilder {
    menus: Vec<MenuItem>,
    entries: Vec<SmallVec<[PathBuf; 2]>>,
    require_existing_paths: bool,
    canonicalize_paths: bool,
}

impl JumpListBuilder {
    /// Create an empty jump-list builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an action item to the jump-list task section.
    pub fn action(mut self, name: impl Into<SharedString>, action: impl Action) -> Self {
        self.menus.push(MenuItem::action(name, action));
        self
    }

    /// Add an already-constructed menu item.
    pub fn menu_item(mut self, item: MenuItem) -> Self {
        self.menus.push(item);
        self
    }

    /// Add one recent workspace entry made from one or more paths.
    pub fn workspace(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        let entry = paths.into_iter().map(Into::into).collect::<SmallVec<_>>();
        if !self.entries.iter().any(|existing| existing == &entry) {
            self.entries.push(entry);
        }
        self
    }

    /// Add one recent workspace entry with a single path.
    pub fn workspace_path(self, path: impl Into<PathBuf>) -> Self {
        self.workspace([path.into()])
    }

    /// Add multiple recent workspace entries.
    pub fn workspaces(
        mut self,
        entries: impl IntoIterator<Item = impl IntoIterator<Item = impl Into<PathBuf>>>,
    ) -> Self {
        for entry in entries {
            self = self.workspace(entry);
        }
        self
    }

    /// Require every workspace path to currently exist.
    pub fn require_existing_paths(mut self) -> Self {
        self.require_existing_paths = true;
        self
    }

    /// Allow missing workspace paths. This is the default and matches the raw API.
    pub fn allow_missing(mut self) -> Self {
        self.require_existing_paths = false;
        self
    }

    /// Canonicalize workspace paths before installing the jump list.
    pub fn canonicalize(mut self) -> Self {
        self.canonicalize_paths = true;
        self
    }

    /// Preserve workspace paths exactly as configured. This is the default.
    pub fn preserve_paths(mut self) -> Self {
        self.canonicalize_paths = false;
        self
    }

    /// Return the configured task menu items.
    pub fn menus(&self) -> &[MenuItem] {
        &self.menus
    }

    /// Return the configured recent workspace entries.
    pub fn entries(&self) -> &[SmallVec<[PathBuf; 2]>] {
        &self.entries
    }

    /// Whether workspace paths must exist before installing the jump list.
    pub fn requires_existing_paths(&self) -> bool {
        self.require_existing_paths
    }

    /// Whether workspace paths will be canonicalized before installing the jump list.
    pub fn canonicalizes_paths(&self) -> bool {
        self.canonicalize_paths
    }

    /// Validate task items and workspace entries before installing the jump list.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.menus.is_empty() || !self.entries.is_empty(),
            "jump list must contain at least one task or workspace entry"
        );

        for item in &self.menus {
            validate_jump_list_menu_item(item)?;
        }
        self.resolve_entries()?;
        Ok(())
    }

    /// Build the validated task items and workspace entries.
    pub fn build_checked(self) -> Result<(Vec<MenuItem>, Vec<SmallVec<[PathBuf; 2]>>)> {
        self.validate()?;
        let entries = self.resolve_entries()?;
        Ok((self.menus, entries))
    }

    fn resolve_entries(&self) -> Result<Vec<SmallVec<[PathBuf; 2]>>> {
        let mut entries = Vec::new();
        for entry in &self.entries {
            anyhow::ensure!(
                !entry.is_empty(),
                "jump-list workspace entry must contain at least one path"
            );

            let mut resolved = SmallVec::<[PathBuf; 2]>::new();
            for path in entry {
                anyhow::ensure!(
                    !path.as_os_str().is_empty(),
                    "jump-list workspace path cannot be empty"
                );
                let path = if self.canonicalize_paths {
                    fs::canonicalize(path).with_context(|| {
                        format!("failed to canonicalize jump-list path {}", path.display())
                    })?
                } else {
                    path.clone()
                };

                if self.require_existing_paths {
                    anyhow::ensure!(
                        path.exists(),
                        "jump-list workspace path must exist: {}",
                        path.display()
                    );
                }

                if !resolved.iter().any(|existing| existing == &path) {
                    resolved.push(path);
                }
            }

            if !entries.iter().any(|existing| existing == &resolved) {
                entries.push(resolved);
            }
        }
        Ok(entries)
    }
}

fn validate_jump_list_menu_item(item: &MenuItem) -> Result<()> {
    match item {
        MenuItem::Action { name, .. } => {
            anyhow::ensure!(
                !name.trim().is_empty(),
                "jump-list action label cannot be empty"
            );
            anyhow::ensure!(
                name.as_ref() == name.trim(),
                "jump-list action label cannot have leading or trailing whitespace"
            );
            Ok(())
        }
        _ => anyhow::bail!("jump-list tasks only support action menu items"),
    }
}

/// Builder for checking and requesting common OS permissions together.
#[derive(Default)]
pub struct PermissionRequestBuilder {
    accessibility: bool,
    microphone: bool,
    camera: bool,
    microphone_callback: Option<Box<dyn FnOnce(bool)>>,
    camera_callback: Option<Box<dyn FnOnce(bool)>>,
}

impl PermissionRequestBuilder {
    /// Create an empty permission request builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check common startup privacy permissions: accessibility, microphone, and camera.
    pub fn startup_privacy() -> Self {
        Self::new().accessibility().media_devices()
    }

    /// Check permissions commonly needed by capture or recording tools.
    pub fn capture_studio() -> Self {
        Self::startup_privacy()
    }

    /// Check accessibility permission and request it when still undetermined.
    pub fn accessibility(mut self) -> Self {
        self.accessibility = true;
        self
    }

    /// Check microphone permission and request it when still undetermined.
    pub fn microphone(mut self) -> Self {
        self.microphone = true;
        self
    }

    /// Check microphone permission and receive the platform prompt result.
    pub fn microphone_with_callback(mut self, callback: impl FnOnce(bool) + 'static) -> Self {
        self.microphone = true;
        self.microphone_callback = Some(Box::new(callback));
        self
    }

    /// Check camera permission and request it when still undetermined.
    pub fn camera(mut self) -> Self {
        self.camera = true;
        self
    }

    /// Check camera permission and receive the platform prompt result.
    pub fn camera_with_callback(mut self, callback: impl FnOnce(bool) + 'static) -> Self {
        self.camera = true;
        self.camera_callback = Some(Box::new(callback));
        self
    }

    /// Check and request both microphone and camera permissions.
    pub fn media_devices(self) -> Self {
        self.microphone().camera()
    }

    /// Return whether accessibility permission is configured.
    pub fn requests_accessibility(&self) -> bool {
        self.accessibility
    }

    /// Return whether microphone permission is configured.
    pub fn requests_microphone(&self) -> bool {
        self.microphone
    }

    /// Return whether camera permission is configured.
    pub fn requests_camera(&self) -> bool {
        self.camera
    }

    /// Validate that at least one permission was configured.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.accessibility || self.microphone || self.camera,
            "at least one permission must be configured"
        );
        Ok(())
    }

    fn into_parts(
        self,
    ) -> (
        bool,
        bool,
        bool,
        Option<Box<dyn FnOnce(bool)>>,
        Option<Box<dyn FnOnce(bool)>>,
    ) {
        (
            self.accessibility,
            self.microphone,
            self.camera,
            self.microphone_callback,
            self.camera_callback,
        )
    }
}

/// A requested permission that is currently denied or restricted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionRequestDenial {
    /// Stable permission key for logs, tests, and settings routing.
    pub key: &'static str,
    /// User-facing permission label.
    pub label: &'static str,
    /// Current OS permission status.
    pub status: PermissionStatus,
}

impl PermissionRequestDenial {
    /// Return whether the denial is caused by system policy.
    pub fn is_restricted(&self) -> bool {
        self.status == PermissionStatus::Restricted
    }
}

/// A requested permission with its current OS status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionRequestStatus {
    /// Stable permission key for logs, tests, and settings routing.
    pub key: &'static str,
    /// User-facing permission label.
    pub label: &'static str,
    /// Current OS permission status.
    pub status: PermissionStatus,
}

/// Status snapshot returned by [`App::request_permissions`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionRequestResult {
    /// Accessibility status before any prompt was launched.
    pub accessibility: Option<PermissionStatus>,
    /// Microphone status before any prompt was launched.
    pub microphone: Option<PermissionStatus>,
    /// Camera status before any prompt was launched.
    pub camera: Option<PermissionStatus>,
    /// Whether Kael launched an accessibility permission prompt.
    pub requested_accessibility: bool,
    /// Whether Kael launched a microphone permission prompt.
    pub requested_microphone: bool,
    /// Whether Kael launched a camera permission prompt.
    pub requested_camera: bool,
}

impl PermissionRequestResult {
    /// Return all requested permissions with their current status.
    pub fn requested_permissions(&self) -> Vec<PermissionRequestStatus> {
        let mut permissions = Vec::new();
        push_permission_status(
            &mut permissions,
            "accessibility",
            "Accessibility",
            self.accessibility,
        );
        push_permission_status(
            &mut permissions,
            "microphone",
            "Microphone",
            self.microphone,
        );
        push_permission_status(&mut permissions, "camera", "Camera", self.camera);
        permissions
    }

    /// Return requested permissions that are currently granted.
    pub fn granted_permissions(&self) -> Vec<PermissionRequestStatus> {
        self.requested_permissions()
            .into_iter()
            .filter(|permission| permission.status == PermissionStatus::Granted)
            .collect()
    }

    /// Return requested permissions that are still pending an OS decision.
    pub fn pending_permissions(&self) -> Vec<PermissionRequestStatus> {
        self.requested_permissions()
            .into_iter()
            .filter(|permission| matches!(permission.status, PermissionStatus::NotDetermined))
            .collect()
    }

    /// Return true if any requested permission still needs an OS decision.
    pub fn has_pending_permission(&self) -> bool {
        !self.pending_permissions().is_empty()
    }

    /// Return true if every requested permission is granted.
    pub fn all_granted(&self) -> bool {
        [self.accessibility, self.microphone, self.camera]
            .into_iter()
            .flatten()
            .all(|status| status == PermissionStatus::Granted)
    }

    /// Return requested permissions that are currently denied or restricted.
    pub fn blocking_denials(&self) -> Vec<PermissionRequestDenial> {
        let mut denials = Vec::new();
        push_permission_denial(
            &mut denials,
            "accessibility",
            "Accessibility",
            self.accessibility,
        );
        push_permission_denial(&mut denials, "microphone", "Microphone", self.microphone);
        push_permission_denial(&mut denials, "camera", "Camera", self.camera);
        denials
    }

    /// Return true if any requested permission is currently denied or restricted.
    pub fn has_blocking_denial(&self) -> bool {
        !self.blocking_denials().is_empty()
    }

    /// Return a compact summary of denied or restricted permissions.
    pub fn blocking_denial_summary(&self) -> Option<String> {
        let denials = self.blocking_denials();
        if denials.is_empty() {
            return None;
        }

        Some(
            denials
                .into_iter()
                .map(|denial| format!("{}: {:?}", denial.label, denial.status))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    /// Return a compact summary of granted permissions.
    pub fn granted_summary(&self) -> Option<String> {
        let granted = self.granted_permissions();
        if granted.is_empty() {
            return None;
        }

        Some(
            granted
                .into_iter()
                .map(|permission| permission.label)
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    /// Return true if any OS prompt was launched.
    pub fn prompted(&self) -> bool {
        self.requested_accessibility || self.requested_microphone || self.requested_camera
    }
}

fn push_permission_status(
    permissions: &mut Vec<PermissionRequestStatus>,
    key: &'static str,
    label: &'static str,
    status: Option<PermissionStatus>,
) {
    if let Some(status) = status {
        permissions.push(PermissionRequestStatus { key, label, status });
    }
}

fn push_permission_denial(
    denials: &mut Vec<PermissionRequestDenial>,
    key: &'static str,
    label: &'static str,
    status: Option<PermissionStatus>,
) {
    if let Some(status @ (PermissionStatus::Denied | PermissionStatus::Restricted)) = status {
        denials.push(PermissionRequestDenial { key, label, status });
    }
}

/// Builder for starting a power-save blocker with app-level intent.
#[derive(Debug, Clone)]
pub struct PowerSaveBlockerBuilder {
    kind: PowerSaveBlockerKind,
    reason: Option<String>,
}

impl PowerSaveBlockerBuilder {
    /// Prevent the app process from being suspended.
    pub fn prevent_app_suspension() -> Self {
        Self {
            kind: PowerSaveBlockerKind::PreventAppSuspension,
            reason: None,
        }
    }

    /// Prevent the display from sleeping while visible work is active.
    pub fn prevent_display_sleep() -> Self {
        Self {
            kind: PowerSaveBlockerKind::PreventDisplaySleep,
            reason: None,
        }
    }

    /// Attach a human-readable reason for logs, diagnostics, and app state.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Return the raw platform blocker kind.
    pub fn kind(&self) -> PowerSaveBlockerKind {
        self.kind
    }

    /// Return the optional reason.
    pub fn configured_reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the configured blocker request.
    pub fn validate(&self) -> Result<()> {
        if let Some(reason) = &self.reason {
            anyhow::ensure!(
                !reason.trim().is_empty(),
                "power-save blocker reason cannot be empty"
            );
        }
        Ok(())
    }

    /// Build the validated blocker request into raw parts.
    pub fn build_checked(self) -> Result<(PowerSaveBlockerKind, Option<String>)> {
        self.validate()?;
        Ok(self.into_parts())
    }

    fn into_parts(self) -> (PowerSaveBlockerKind, Option<String>) {
        (self.kind, self.reason)
    }
}

impl From<PowerSaveBlockerKind> for PowerSaveBlockerBuilder {
    fn from(value: PowerSaveBlockerKind) -> Self {
        Self {
            kind: value,
            reason: None,
        }
    }
}

/// Active power-save blocker returned by [`App::start_power_save_blocker_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerSaveBlockerHandle {
    id: u32,
    kind: PowerSaveBlockerKind,
    reason: Option<String>,
}

impl PowerSaveBlockerHandle {
    /// Platform-specific blocker ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The blocker kind that was requested.
    pub fn kind(&self) -> PowerSaveBlockerKind {
        self.kind
    }

    /// Optional human-readable reason supplied by the app.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Stop this blocker.
    pub fn stop(self, app: &App) {
        app.stop_power_save_blocker(self.id);
    }
}

/// Builder for requesting user attention from the OS.
#[derive(Debug, Clone)]
pub struct UserAttentionBuilder {
    attention_type: AttentionType,
    reason: Option<String>,
}

impl UserAttentionBuilder {
    /// Request informational attention, such as a short dock bounce or taskbar flash.
    pub fn informational() -> Self {
        Self {
            attention_type: AttentionType::Informational,
            reason: None,
        }
    }

    /// Request critical attention, such as continuous bouncing or flashing.
    pub fn critical() -> Self {
        Self {
            attention_type: AttentionType::Critical,
            reason: None,
        }
    }

    /// Attach a human-readable reason for logs, diagnostics, or app state.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Return the raw attention type.
    pub fn attention_type(&self) -> AttentionType {
        self.attention_type
    }

    /// Return the optional reason.
    pub fn configured_reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the configured attention request.
    pub fn validate(&self) -> Result<()> {
        if let Some(reason) = &self.reason {
            anyhow::ensure!(
                !reason.trim().is_empty(),
                "user attention reason cannot be empty"
            );
        }
        Ok(())
    }

    /// Build the validated attention request into raw parts.
    pub fn build_checked(self) -> Result<(AttentionType, Option<String>)> {
        self.validate()?;
        Ok(self.into_parts())
    }

    fn into_parts(self) -> (AttentionType, Option<String>) {
        (self.attention_type, self.reason)
    }
}

impl From<AttentionType> for UserAttentionBuilder {
    fn from(value: AttentionType) -> Self {
        Self {
            attention_type: value,
            reason: None,
        }
    }
}

/// Active user-attention request returned by [`App::request_user_attention_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAttentionRequest {
    attention_type: AttentionType,
    reason: Option<String>,
}

impl UserAttentionRequest {
    /// The requested attention type.
    pub fn attention_type(&self) -> AttentionType {
        self.attention_type
    }

    /// Optional human-readable reason supplied by the app.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Cancel the current OS attention request.
    pub fn cancel(self, app: &App) {
        app.cancel_user_attention();
    }
}

/// Temporary(?) wrapper around [`RefCell<App>`] to help us debug any double borrows.
/// Strongly consider removing after stabilization.
#[doc(hidden)]
pub struct AppCell {
    app: RefCell<App>,
}

impl AppCell {
    #[doc(hidden)]
    #[track_caller]
    pub fn borrow(&self) -> AppRef<'_> {
        if option_env!("TRACK_THREAD_BORROWS").is_some() {
            let thread_id = std::thread::current().id();
            eprintln!("borrowed {thread_id:?}");
        }
        AppRef(self.app.borrow())
    }

    #[doc(hidden)]
    #[track_caller]
    pub fn borrow_mut(&self) -> AppRefMut<'_> {
        if option_env!("TRACK_THREAD_BORROWS").is_some() {
            let thread_id = std::thread::current().id();
            eprintln!("borrowed {thread_id:?}");
        }
        AppRefMut(self.app.borrow_mut())
    }

    #[doc(hidden)]
    #[track_caller]
    pub fn try_borrow_mut(&self) -> Result<AppRefMut<'_>, BorrowMutError> {
        if option_env!("TRACK_THREAD_BORROWS").is_some() {
            let thread_id = std::thread::current().id();
            eprintln!("borrowed {thread_id:?}");
        }
        Ok(AppRefMut(self.app.try_borrow_mut()?))
    }

    /// Register a handler to be invoked when the platform instructs the application
    /// to open one or more URLs.
    pub fn on_open_urls<F>(&self, mut callback: F) -> &Self
    where
        F: 'static + FnMut(Vec<String>),
    {
        self.app
            .borrow_mut()
            .open_urls_observers
            .push(Box::new(move |urls| callback(urls)));
        self
    }

    /// Register a handler to be invoked for each URL the platform asks the app to open.
    pub fn on_open_url<F>(&self, callback: F) -> &Self
    where
        F: 'static + FnMut(String, &mut App),
    {
        self.app
            .borrow_mut()
            .open_url_observers
            .push(Box::new(callback));
        self
    }

    /// Register a handler to be invoked when the platform instructs the application
    /// to open one or more typed requests.
    pub fn on_open_requests<F>(&self, mut callback: F) -> &Self
    where
        F: 'static + FnMut(Vec<OpenRequest>),
    {
        self.app
            .borrow_mut()
            .open_requests_observers
            .push(Box::new(move |requests| callback(requests)));
        self
    }

    /// Register a handler for each typed open request the platform delivers.
    pub fn on_open_request<F>(&self, callback: F) -> &Self
    where
        F: 'static + FnMut(OpenRequest, &mut App),
    {
        self.app
            .borrow_mut()
            .open_request_observers
            .push(Box::new(callback));
        self
    }

    /// Register a handler for URLs matching a specific scheme.
    pub fn on_deep_link<F>(&self, scheme: impl Into<String>, callback: F) -> &Self
    where
        F: 'static + FnMut(String, &mut App),
    {
        self.app
            .borrow_mut()
            .deep_link_observers
            .entry(scheme.into())
            .or_default()
            .push(Box::new(callback));
        self
    }

    /// Register grouped deep-link routes.
    pub fn deep_links(&self, routes: impl Into<Vec<DeepLinkRoute>>) -> &Self {
        let mut app = self.app.borrow_mut();
        for route in routes.into() {
            let (scheme, callback) = route.into_parts();
            app.deep_link_observers
                .entry(scheme)
                .or_default()
                .push(callback);
        }
        self
    }

    /// Validate and register grouped deep-link routes.
    pub fn deep_links_checked(&self, routes: DeepLinkRouterBuilder) -> Result<&Self> {
        let mut app = self.app.borrow_mut();
        for route in routes.build_checked()? {
            let (scheme, callback) = route.into_parts();
            app.deep_link_observers
                .entry(scheme)
                .or_default()
                .push(callback);
        }
        Ok(self)
    }

    /// Register grouped custom protocol routes.
    pub fn custom_protocols(&self, routes: impl Into<Vec<CustomProtocolRoute>>) -> &Self {
        let mut app = self.app.borrow_mut();
        for route in routes.into() {
            let (scheme, handler) = route.into_parts();
            app.custom_protocol_handlers.insert(scheme, handler);
        }
        self
    }

    /// Validate and register grouped custom protocol routes.
    pub fn custom_protocols_checked(&self, routes: CustomProtocolRouterBuilder) -> Result<&Self> {
        let mut app = self.app.borrow_mut();
        for route in routes.build_checked()? {
            let (scheme, handler) = route.into_parts();
            app.custom_protocol_handlers.insert(scheme, handler);
        }
        Ok(self)
    }
}

#[doc(hidden)]
#[derive(Deref, DerefMut)]
pub struct AppRef<'a>(Ref<'a, App>);

impl Drop for AppRef<'_> {
    fn drop(&mut self) {
        if option_env!("TRACK_THREAD_BORROWS").is_some() {
            let thread_id = std::thread::current().id();
            eprintln!("dropped borrow from {thread_id:?}");
        }
    }
}

#[doc(hidden)]
#[derive(Deref, DerefMut)]
pub struct AppRefMut<'a>(RefMut<'a, App>);

impl Drop for AppRefMut<'_> {
    fn drop(&mut self) {
        if option_env!("TRACK_THREAD_BORROWS").is_some() {
            let thread_id = std::thread::current().id();
            eprintln!("dropped {thread_id:?}");
        }
    }
}

/// A reference to a GPUI application, typically constructed in the `main` function of your app.
/// You won't interact with this type much outside of initial configuration and startup.
pub struct Application(Rc<AppCell>);

/// Represents an application before it is fully launched. Once your app is
/// configured, you'll start the app with `App::run`.
impl Application {
    /// Builds an app with the given asset source.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[cfg(any(test, feature = "test-support"))]
        log::info!("GPUI was compiled in test mode");

        Self(App::new_app(
            current_platform(false),
            Arc::new(()),
            Arc::new(NullHttpClient),
        ))
    }

    /// Build an app in headless mode. This prevents opening windows,
    /// but makes it possible to run an application in an context like
    /// SSH, where GUI applications are not allowed.
    pub fn headless() -> Self {
        Self(App::new_app(
            current_platform(true),
            Arc::new(()),
            Arc::new(NullHttpClient),
        ))
    }

    /// Assign
    pub fn with_assets(self, asset_source: impl AssetSource) -> Self {
        let mut context_lock = self.0.borrow_mut();
        let asset_source = Arc::new(asset_source);
        context_lock.asset_source = asset_source.clone();
        context_lock.svg_renderer = SvgRenderer::new(asset_source);
        drop(context_lock);
        self
    }

    /// Sets the HTTP client for the application.
    pub fn with_http_client(self, http_client: Arc<dyn HttpClient>) -> Self {
        let mut context_lock = self.0.borrow_mut();
        context_lock.http_client = http_client;
        drop(context_lock);
        self
    }

    /// Start the application. The provided callback will be called once the
    /// app is fully launched.
    pub fn run<F>(self, on_finish_launching: F)
    where
        F: 'static + FnOnce(&mut App),
    {
        let this = self.0.clone();
        let platform = self.0.borrow().platform.clone();
        platform.run(Box::new(move || {
            let cx = &mut *this.borrow_mut();
            on_finish_launching(cx);
        }));
    }

    /// Register a handler to be invoked when the platform instructs the application
    /// to open one or more URLs.
    pub fn on_open_urls<F>(&self, mut callback: F) -> &Self
    where
        F: 'static + FnMut(Vec<String>),
    {
        self.0.on_open_urls(move |urls| callback(urls));
        self
    }

    /// Register a handler to be invoked for each URL the platform asks the app to open.
    pub fn on_open_url<F>(&self, callback: F) -> &Self
    where
        F: 'static + FnMut(String, &mut App),
    {
        self.0.on_open_url(callback);
        self
    }

    /// Register a handler to be invoked when the platform instructs the application
    /// to open one or more typed requests.
    pub fn on_open_requests<F>(&self, mut callback: F) -> &Self
    where
        F: 'static + FnMut(Vec<OpenRequest>),
    {
        self.0.on_open_requests(move |requests| callback(requests));
        self
    }

    /// Register a handler for each typed open request the platform delivers.
    pub fn on_open_request<F>(&self, callback: F) -> &Self
    where
        F: 'static + FnMut(OpenRequest, &mut App),
    {
        self.0.on_open_request(callback);
        self
    }

    /// Register a handler for URLs matching a specific scheme.
    pub fn on_deep_link<F>(&self, scheme: impl Into<String>, callback: F) -> &Self
    where
        F: 'static + FnMut(String, &mut App),
    {
        self.0.on_deep_link(scheme, callback);
        self
    }

    /// Register grouped deep-link routes.
    pub fn deep_links(&self, routes: impl Into<Vec<DeepLinkRoute>>) -> &Self {
        self.0.deep_links(routes);
        self
    }

    /// Validate and register grouped deep-link routes.
    pub fn deep_links_checked(&self, routes: DeepLinkRouterBuilder) -> Result<&Self> {
        self.0.deep_links_checked(routes)?;
        Ok(self)
    }

    /// Register grouped custom protocol routes.
    pub fn custom_protocols(&self, routes: impl Into<Vec<CustomProtocolRoute>>) -> &Self {
        self.0.custom_protocols(routes);
        self
    }

    /// Validate and register grouped custom protocol routes.
    pub fn custom_protocols_checked(&self, routes: CustomProtocolRouterBuilder) -> Result<&Self> {
        self.0.custom_protocols_checked(routes)?;
        Ok(self)
    }

    /// Invokes a handler when an already-running application is launched.
    /// On macOS, this can occur when the application icon is double-clicked or the app is launched via the dock.
    pub fn on_reopen<F>(&self, mut callback: F) -> &Self
    where
        F: 'static + FnMut(&mut App),
    {
        let this = Rc::downgrade(&self.0);
        self.0.borrow_mut().platform.on_reopen(Box::new(move || {
            if let Some(app) = this.upgrade() {
                callback(&mut app.borrow_mut());
            }
        }));
        self
    }

    /// Returns a handle to the [`BackgroundExecutor`] associated with this app, which can be used to spawn futures in the background.
    pub fn background_executor(&self) -> BackgroundExecutor {
        self.0.borrow().background_executor.clone()
    }

    /// Returns a handle to the [`ForegroundExecutor`] associated with this app, which can be used to spawn futures in the foreground.
    pub fn foreground_executor(&self) -> ForegroundExecutor {
        self.0.borrow().foreground_executor.clone()
    }

    /// Returns a reference to the [`TextSystem`] associated with this app.
    pub fn text_system(&self) -> Arc<TextSystem> {
        self.0.borrow().text_system.clone()
    }

    /// Returns the file URL of the executable with the specified name in the application bundle
    pub fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        self.0.borrow().path_for_auxiliary_executable(name)
    }
}

type Handler = Box<dyn FnMut(&mut App) -> bool + 'static>;
type Listener = Box<dyn FnMut(&dyn Any, &mut App) -> bool + 'static>;
pub(crate) type KeystrokeObserver =
    Box<dyn FnMut(&KeystrokeEvent, &mut Window, &mut App) -> bool + 'static>;
type QuitHandler = Box<dyn FnOnce(&mut App) -> LocalBoxFuture<'static, ()> + 'static>;
type WindowClosedHandler = Box<dyn FnMut(&mut App)>;
type SystemPowerHandler = Box<dyn FnMut(SystemPowerEvent, &mut App) + 'static>;
type ReleaseListener = Box<dyn FnOnce(&mut dyn Any, &mut App) + 'static>;
type NewEntityListener = Box<dyn FnMut(AnyEntity, &mut Option<&mut Window>, &mut App) + 'static>;

#[doc(hidden)]
#[derive(Clone, PartialEq, Eq)]
pub struct SystemWindowTab {
    pub id: WindowId,
    pub title: SharedString,
    pub handle: AnyWindowHandle,
    pub last_active_at: Instant,
}

impl SystemWindowTab {
    /// Create a new instance of the window tab.
    pub fn new(title: SharedString, handle: AnyWindowHandle) -> Self {
        Self {
            id: handle.id,
            title,
            handle,
            last_active_at: Instant::now(),
        }
    }
}

/// A controller for managing window tabs.
#[derive(Default)]
pub struct SystemWindowTabController {
    visible: Option<bool>,
    tab_groups: FxHashMap<usize, Vec<SystemWindowTab>>,
}

impl Global for SystemWindowTabController {}

impl SystemWindowTabController {
    /// Create a new instance of the window tab controller.
    pub fn new() -> Self {
        Self {
            visible: None,
            tab_groups: FxHashMap::default(),
        }
    }

    /// Initialize the global window tab controller.
    pub fn init(cx: &mut App) {
        cx.set_global(SystemWindowTabController::new());
    }

    /// Get all tab groups.
    pub fn tab_groups(&self) -> &FxHashMap<usize, Vec<SystemWindowTab>> {
        &self.tab_groups
    }

    /// Get the next tab group window handle.
    pub fn get_next_tab_group_window(cx: &mut App, id: WindowId) -> Option<&AnyWindowHandle> {
        let controller = cx.global::<SystemWindowTabController>();
        let current_group = controller
            .tab_groups
            .iter()
            .find_map(|(group, tabs)| tabs.iter().find(|tab| tab.id == id).map(|_| group));

        let current_group = current_group?;
        let mut group_ids: Vec<_> = controller.tab_groups.keys().collect();
        let idx = group_ids.iter().position(|g| *g == current_group)?;
        let next_idx = (idx + 1) % group_ids.len();

        controller
            .tab_groups
            .get(group_ids[next_idx])
            .and_then(|tabs| {
                tabs.iter()
                    .max_by_key(|tab| tab.last_active_at)
                    .or_else(|| tabs.first())
                    .map(|tab| &tab.handle)
            })
    }

    /// Get the previous tab group window handle.
    pub fn get_prev_tab_group_window(cx: &mut App, id: WindowId) -> Option<&AnyWindowHandle> {
        let controller = cx.global::<SystemWindowTabController>();
        let current_group = controller
            .tab_groups
            .iter()
            .find_map(|(group, tabs)| tabs.iter().find(|tab| tab.id == id).map(|_| group));

        let current_group = current_group?;
        let mut group_ids: Vec<_> = controller.tab_groups.keys().collect();
        let idx = group_ids.iter().position(|g| *g == current_group)?;
        let prev_idx = if idx == 0 {
            group_ids.len() - 1
        } else {
            idx - 1
        };

        controller
            .tab_groups
            .get(group_ids[prev_idx])
            .and_then(|tabs| {
                tabs.iter()
                    .max_by_key(|tab| tab.last_active_at)
                    .or_else(|| tabs.first())
                    .map(|tab| &tab.handle)
            })
    }

    /// Get all tabs in the same window.
    pub fn tabs(&self, id: WindowId) -> Option<&Vec<SystemWindowTab>> {
        let tab_group = self
            .tab_groups
            .iter()
            .find_map(|(group, tabs)| tabs.iter().find(|tab| tab.id == id).map(|_| *group));

        if let Some(tab_group) = tab_group {
            self.tab_groups.get(&tab_group)
        } else {
            None
        }
    }

    /// Initialize the visibility of the system window tab controller.
    pub fn init_visible(cx: &mut App, visible: bool) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        if controller.visible.is_none() {
            controller.visible = Some(visible);
        }
    }

    /// Get the visibility of the system window tab controller.
    pub fn is_visible(&self) -> bool {
        self.visible.unwrap_or(false)
    }

    /// Set the visibility of the system window tab controller.
    pub fn set_visible(cx: &mut App, visible: bool) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        controller.visible = Some(visible);
    }

    /// Update the last active of a window.
    pub fn update_last_active(cx: &mut App, id: WindowId) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        for windows in controller.tab_groups.values_mut() {
            for tab in windows.iter_mut() {
                if tab.id == id {
                    tab.last_active_at = Instant::now();
                }
            }
        }
    }

    /// Update the position of a tab within its group.
    pub fn update_tab_position(cx: &mut App, id: WindowId, ix: usize) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        for (_, windows) in controller.tab_groups.iter_mut() {
            if let Some(current_pos) = windows.iter().position(|tab| tab.id == id) {
                if ix < windows.len() && current_pos != ix {
                    let window_tab = windows.remove(current_pos);
                    windows.insert(ix, window_tab);
                }
                break;
            }
        }
    }

    /// Update the title of a tab.
    pub fn update_tab_title(cx: &mut App, id: WindowId, title: SharedString) {
        let controller = cx.global::<SystemWindowTabController>();
        let tab = controller
            .tab_groups
            .values()
            .flat_map(|windows| windows.iter())
            .find(|tab| tab.id == id);

        if tab.map_or(true, |t| t.title == title) {
            return;
        }

        let mut controller = cx.global_mut::<SystemWindowTabController>();
        for windows in controller.tab_groups.values_mut() {
            for tab in windows.iter_mut() {
                if tab.id == id {
                    tab.title = title.clone();
                }
            }
        }
    }

    /// Insert a tab into a tab group.
    pub fn add_tab(cx: &mut App, id: WindowId, tabs: Vec<SystemWindowTab>) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        let Some(tab) = tabs.clone().into_iter().find(|tab| tab.id == id) else {
            return;
        };

        let mut expected_tab_ids: Vec<_> = tabs
            .iter()
            .filter(|tab| tab.id != id)
            .map(|tab| tab.id)
            .sorted()
            .collect();

        let mut tab_group_id = None;
        for (group_id, group_tabs) in &controller.tab_groups {
            let tab_ids: Vec<_> = group_tabs.iter().map(|tab| tab.id).sorted().collect();
            if tab_ids == expected_tab_ids {
                tab_group_id = Some(*group_id);
                break;
            }
        }

        if let Some(tab_group_id) = tab_group_id {
            if let Some(tabs) = controller.tab_groups.get_mut(&tab_group_id) {
                tabs.push(tab);
            }
        } else {
            let new_group_id = controller.tab_groups.len();
            controller.tab_groups.insert(new_group_id, tabs);
        }
    }

    /// Remove a tab from a tab group.
    pub fn remove_tab(cx: &mut App, id: WindowId) -> Option<SystemWindowTab> {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        let mut removed_tab = None;

        controller.tab_groups.retain(|_, tabs| {
            if let Some(pos) = tabs.iter().position(|tab| tab.id == id) {
                removed_tab = Some(tabs.remove(pos));
            }
            !tabs.is_empty()
        });

        removed_tab
    }

    /// Move a tab to a new tab group.
    pub fn move_tab_to_new_window(cx: &mut App, id: WindowId) {
        let mut removed_tab = Self::remove_tab(cx, id);
        let mut controller = cx.global_mut::<SystemWindowTabController>();

        if let Some(tab) = removed_tab {
            let new_group_id = controller.tab_groups.keys().max().map_or(0, |k| k + 1);
            controller.tab_groups.insert(new_group_id, vec![tab]);
        }
    }

    /// Merge all tab groups into a single group.
    pub fn merge_all_windows(cx: &mut App, id: WindowId) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        let Some(initial_tabs) = controller.tabs(id) else {
            return;
        };

        let mut all_tabs = initial_tabs.clone();
        for tabs in controller.tab_groups.values() {
            all_tabs.extend(
                tabs.iter()
                    .filter(|tab| !initial_tabs.contains(tab))
                    .cloned(),
            );
        }

        controller.tab_groups.clear();
        controller.tab_groups.insert(0, all_tabs);
    }

    /// Selects the next tab in the tab group in the trailing direction.
    pub fn select_next_tab(cx: &mut App, id: WindowId) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        let Some(tabs) = controller.tabs(id) else {
            return;
        };

        let Some(current_index) = tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let next_index = (current_index + 1) % tabs.len();

        let _ = &tabs[next_index].handle.update(cx, |_, window, _| {
            window.activate_window();
        });
    }

    /// Selects the previous tab in the tab group in the leading direction.
    pub fn select_previous_tab(cx: &mut App, id: WindowId) {
        let mut controller = cx.global_mut::<SystemWindowTabController>();
        let Some(tabs) = controller.tabs(id) else {
            return;
        };

        let Some(current_index) = tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let previous_index = if current_index == 0 {
            tabs.len() - 1
        } else {
            current_index - 1
        };

        let _ = &tabs[previous_index].handle.update(cx, |_, window, _| {
            window.activate_window();
        });
    }
}

/// Contains the state of the full application, and passed as a reference to a variety of callbacks.
/// Other [Context] derefs to this type.
/// You need a reference to an `App` to access the state of a [Entity].
pub struct App {
    pub(crate) this: Weak<AppCell>,
    pub(crate) platform: Rc<dyn Platform>,
    text_system: Arc<TextSystem>,
    flushing_effects: bool,
    pending_updates: usize,
    pub(crate) actions: Rc<ActionRegistry>,
    pub(crate) active_drag: Option<AnyDrag>,
    pub(crate) background_executor: BackgroundExecutor,
    pub(crate) foreground_executor: ForegroundExecutor,
    started_at: Instant,
    pub(crate) loading_assets: FxHashMap<(TypeId, u64), Box<dyn Any>>,
    asset_source: Arc<dyn AssetSource>,
    pub(crate) svg_renderer: SvgRenderer,
    http_client: Arc<dyn HttpClient>,
    pub(crate) globals_by_type: FxHashMap<TypeId, Box<dyn Any>>,
    global_generation: u64,
    pub(crate) entities: EntityMap,
    pub(crate) window_update_stack: Vec<WindowId>,
    pub(crate) new_entity_observers: SubscriberSet<TypeId, NewEntityListener>,
    pub(crate) windows: SlotMap<WindowId, Option<Window>>,
    pub(crate) window_handles: FxHashMap<WindowId, AnyWindowHandle>,
    pub(crate) focus_handles: Arc<FocusMap>,
    pub(crate) keymap: Rc<RefCell<Keymap>>,
    pub(crate) keyboard_layout: Box<dyn PlatformKeyboardLayout>,
    pub(crate) keyboard_mapper: Rc<dyn PlatformKeyboardMapper>,
    pub(crate) global_action_listeners:
        FxHashMap<TypeId, Vec<Rc<dyn Fn(&dyn Any, DispatchPhase, &mut Self)>>>,
    pending_effects: VecDeque<Effect>,
    pub(crate) pending_notifications: FxHashSet<EntityId>,
    pub(crate) pending_global_notifications: FxHashSet<TypeId>,
    pub(crate) observers: SubscriberSet<EntityId, Handler>,
    // TypeId is the type of the event that the listener callback expects
    pub(crate) event_listeners: SubscriberSet<EntityId, (TypeId, Listener)>,
    pub(crate) keystroke_observers: SubscriberSet<(), KeystrokeObserver>,
    pub(crate) keystroke_interceptors: SubscriberSet<(), KeystrokeObserver>,
    pub(crate) keyboard_layout_observers: SubscriberSet<(), Handler>,
    pub(crate) release_listeners: SubscriberSet<EntityId, ReleaseListener>,
    pub(crate) global_observers: SubscriberSet<TypeId, Handler>,
    pub(crate) quit_observers: SubscriberSet<(), QuitHandler>,
    pub(crate) restart_observers: SubscriberSet<(), Handler>,
    pub(crate) restart_path: Option<PathBuf>,
    shutdown_timeout: Duration,
    keep_alive_without_windows: bool,
    pub(crate) window_closed_observers: SubscriberSet<(), WindowClosedHandler>,
    pub(crate) layout_id_buffer: Vec<LayoutId>, // We recycle this memory across layout requests.
    pub(crate) propagate_event: bool,
    pub(crate) prompt_builder: Option<PromptBuilder>,
    system_power_handler: Option<SystemPowerHandler>,
    pub(crate) window_invalidators_by_entity:
        FxHashMap<EntityId, FxHashMap<WindowId, WindowInvalidator>>,
    pub(crate) tracked_entities: FxHashMap<WindowId, FxHashSet<EntityId>>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) inspector_renderer: Option<crate::InspectorRenderer>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) inspector_element_registry: InspectorElementRegistry,
    #[cfg(any(test, feature = "test-support", debug_assertions))]
    pub(crate) name: Option<&'static str>,
    quitting: bool,
    pub(crate) workspace: Option<crate::workspace::Workspace>,
    pub(crate) command_registry: crate::CommandRegistry,
    pub(crate) background_jobs: crate::background_jobs::JobScheduler,
    open_urls_observers: Vec<Box<dyn FnMut(Vec<String>)>>,
    open_url_observers: Vec<Box<dyn FnMut(String, &mut App)>>,
    open_requests_observers: Vec<Box<dyn FnMut(Vec<OpenRequest>)>>,
    open_request_observers: Vec<Box<dyn FnMut(OpenRequest, &mut App)>>,
    deep_link_observers: HashMap<String, Vec<Box<dyn FnMut(String, &mut App)>>>,
    custom_protocol_handlers: HashMap<String, CustomProtocolHandler>,
    pub(crate) deep_link_registry: crate::DeepLinkRegistry,
    pub(crate) permission_broker: PermissionBroker,
    pub(crate) current_process_id: ProcessId,
}

impl App {
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new_app(
        platform: Rc<dyn Platform>,
        asset_source: Arc<dyn AssetSource>,
        http_client: Arc<dyn HttpClient>,
    ) -> Rc<AppCell> {
        let executor = platform.background_executor();
        let foreground_executor = platform.foreground_executor();
        assert!(
            executor.is_main_thread(),
            "must construct App on main thread"
        );

        let text_system = Arc::new(TextSystem::new(platform.text_system()));
        let entities = EntityMap::new();
        let keyboard_layout = platform.keyboard_layout();
        let keyboard_mapper = platform.keyboard_mapper();

        let mut permission_broker = PermissionBroker::new();
        let current_process_id = ProcessId(0);
        permission_broker.register_process(current_process_id, ProcessClass::Ui);
        permission_broker.apply_threat_model(&ThreatModel::new());

        let app = Rc::new_cyclic(|this| AppCell {
            app: RefCell::new(App {
                this: this.clone(),
                platform: platform.clone(),
                text_system,
                actions: Rc::new(ActionRegistry::default()),
                flushing_effects: false,
                pending_updates: 0,
                active_drag: None,
                background_executor: executor,
                foreground_executor,
                started_at: Instant::now(),
                svg_renderer: SvgRenderer::new(asset_source.clone()),
                loading_assets: Default::default(),
                asset_source,
                http_client,
                globals_by_type: FxHashMap::default(),
                global_generation: 0,
                entities,
                new_entity_observers: SubscriberSet::new(),
                windows: SlotMap::with_key(),
                window_update_stack: Vec::new(),
                window_handles: FxHashMap::default(),
                focus_handles: Arc::new(RwLock::new(SlotMap::with_key())),
                keymap: Rc::new(RefCell::new(Keymap::default())),
                keyboard_layout,
                keyboard_mapper,
                global_action_listeners: FxHashMap::default(),
                pending_effects: VecDeque::new(),
                pending_notifications: FxHashSet::default(),
                pending_global_notifications: FxHashSet::default(),
                observers: SubscriberSet::new(),
                tracked_entities: FxHashMap::default(),
                window_invalidators_by_entity: FxHashMap::default(),
                event_listeners: SubscriberSet::new(),
                release_listeners: SubscriberSet::new(),
                keystroke_observers: SubscriberSet::new(),
                keystroke_interceptors: SubscriberSet::new(),
                keyboard_layout_observers: SubscriberSet::new(),
                global_observers: SubscriberSet::new(),
                quit_observers: SubscriberSet::new(),
                restart_observers: SubscriberSet::new(),
                restart_path: None,
                shutdown_timeout: SHUTDOWN_TIMEOUT,
                keep_alive_without_windows: false,
                window_closed_observers: SubscriberSet::new(),
                layout_id_buffer: Default::default(),
                propagate_event: true,
                prompt_builder: Some(PromptBuilder::Default),
                system_power_handler: None,
                #[cfg(any(feature = "inspector", debug_assertions))]
                inspector_renderer: None,
                #[cfg(any(feature = "inspector", debug_assertions))]
                inspector_element_registry: InspectorElementRegistry::default(),
                quitting: false,

                #[cfg(any(test, feature = "test-support", debug_assertions))]
                name: None,
                permission_broker,
                current_process_id,
                workspace: None,
                command_registry: CommandRegistry::new(),
                background_jobs: crate::background_jobs::JobScheduler::new(),
                open_urls_observers: Vec::new(),
                open_url_observers: Vec::new(),
                open_requests_observers: Vec::new(),
                open_request_observers: Vec::new(),
                deep_link_observers: HashMap::default(),
                custom_protocol_handlers: HashMap::default(),
                deep_link_registry: crate::DeepLinkRegistry::new(),
            }),
        });

        let weak_app = Rc::downgrade(&app);
        platform.on_open_urls(Box::new(move |urls| {
            let Some(app) = weak_app.upgrade() else {
                return;
            };

            app.borrow_mut().handle_open_urls(urls);
        }));

        init_app_menus(platform.as_ref(), &app.borrow());
        SystemWindowTabController::init(&mut app.borrow_mut());
        Theme::init(&mut app.borrow_mut());

        platform.on_keyboard_layout_change(Box::new({
            let app = Rc::downgrade(&app);
            move || {
                if let Some(app) = app.upgrade() {
                    let cx = &mut app.borrow_mut();
                    cx.keyboard_layout = cx.platform.keyboard_layout();
                    cx.keyboard_mapper = cx.platform.keyboard_mapper();
                    cx.keyboard_layout_observers
                        .clone()
                        .retain(&(), move |callback| (callback)(cx));
                }
            }
        }));

        platform.on_system_power_event(Box::new({
            let app = Rc::downgrade(&app);
            move |event| {
                if let Some(app) = app.upgrade() {
                    let cx = &mut app.borrow_mut();
                    for window in cx.windows.values_mut().flatten() {
                        window.refresh();
                    }

                    if let Some(mut callback) = cx.system_power_handler.take() {
                        callback(event, cx);
                        cx.system_power_handler = Some(callback);
                    }
                }
            }
        }));

        platform.on_quit(Box::new({
            let cx = app.clone();
            move || {
                cx.borrow_mut().shutdown();
            }
        }));

        app
    }

    /// Quit the application gracefully. Handlers registered with [`Context::on_app_quit`]
    /// will be given 100ms to complete before exiting.
    pub fn shutdown(&mut self) {
        let mut futures = Vec::new();

        for observer in self.quit_observers.remove(&()) {
            futures.push(observer(self));
        }

        self.windows.clear();
        self.window_handles.clear();
        self.flush_effects();
        self.quitting = true;

        let futures = futures::future::join_all(futures);
        if self
            .background_executor
            .block_with_timeout(self.shutdown_timeout, futures)
            .is_err()
        {
            log::error!("timed out waiting on app_will_quit");
        }

        self.quitting = false;
    }

    /// Get the id of the current keyboard layout
    pub fn keyboard_layout(&self) -> &dyn PlatformKeyboardLayout {
        self.keyboard_layout.as_ref()
    }

    /// Get the current keyboard mapper.
    pub fn keyboard_mapper(&self) -> &Rc<dyn PlatformKeyboardMapper> {
        &self.keyboard_mapper
    }

    /// Invokes a handler when the current keyboard layout changes
    pub fn on_keyboard_layout_change<F>(&self, mut callback: F) -> Subscription
    where
        F: 'static + FnMut(&mut App),
    {
        let (subscription, activate) = self.keyboard_layout_observers.insert(
            (),
            Box::new(move |cx| {
                callback(cx);
                true
            }),
        );
        activate();
        subscription
    }

    /// Gracefully quit the application via the platform's standard routine.
    pub fn quit(&self) {
        self.platform.quit();
    }

    /// Schedules all windows in the application to be redrawn. This can be called
    /// multiple times in an update cycle and still result in a single redraw.
    pub fn refresh_windows(&mut self) {
        self.pending_effects.push_back(Effect::RefreshWindows);
    }

    pub(crate) fn update<R>(&mut self, update: impl FnOnce(&mut Self) -> R) -> R {
        self.start_update();
        let result = update(self);
        self.finish_update();
        result
    }

    pub(crate) fn start_update(&mut self) {
        self.pending_updates += 1;
    }

    pub(crate) fn finish_update(&mut self) {
        if !self.flushing_effects && self.pending_updates == 1 {
            self.flushing_effects = true;
            self.flush_effects();
            self.flushing_effects = false;
        }
        self.pending_updates -= 1;
    }

    /// Arrange a callback to be invoked when the given entity calls `notify` on its respective context.
    pub fn observe<W>(
        &mut self,
        entity: &Entity<W>,
        mut on_notify: impl FnMut(Entity<W>, &mut App) + 'static,
    ) -> Subscription
    where
        W: 'static,
    {
        self.observe_internal(entity, move |e, cx| {
            on_notify(e, cx);
            true
        })
    }

    pub(crate) fn detect_accessed_entities<R>(
        &mut self,
        callback: impl FnOnce(&mut App) -> R,
    ) -> (R, FxHashSet<EntityId>) {
        let accessed_entities_start = self.entities.accessed_entities.borrow().clone();
        let result = callback(self);
        let accessed_entities_end = self.entities.accessed_entities.borrow().clone();
        let entities_accessed_in_callback = accessed_entities_end
            .difference(&accessed_entities_start)
            .copied()
            .collect::<FxHashSet<EntityId>>();
        (result, entities_accessed_in_callback)
    }

    pub(crate) fn record_entities_accessed(
        &mut self,
        window_handle: AnyWindowHandle,
        invalidator: WindowInvalidator,
        entities: &FxHashSet<EntityId>,
    ) {
        let mut tracked_entities =
            std::mem::take(self.tracked_entities.entry(window_handle.id).or_default());
        for entity in tracked_entities.iter() {
            self.window_invalidators_by_entity
                .entry(*entity)
                .and_modify(|windows| {
                    windows.remove(&window_handle.id);
                });
        }
        for entity in entities.iter() {
            self.window_invalidators_by_entity
                .entry(*entity)
                .or_default()
                .insert(window_handle.id, invalidator.clone());
        }
        tracked_entities.clear();
        tracked_entities.extend(entities.iter().copied());
        self.tracked_entities
            .insert(window_handle.id, tracked_entities);
    }

    pub(crate) fn new_observer(&mut self, key: EntityId, value: Handler) -> Subscription {
        let (subscription, activate) = self.observers.insert(key, value);
        self.defer(move |_| activate());
        subscription
    }

    pub(crate) fn observe_internal<W>(
        &mut self,
        entity: &Entity<W>,
        mut on_notify: impl FnMut(Entity<W>, &mut App) -> bool + 'static,
    ) -> Subscription
    where
        W: 'static,
    {
        let entity_id = entity.entity_id();
        let handle = entity.downgrade();
        self.new_observer(
            entity_id,
            Box::new(move |cx| {
                if let Some(entity) = handle.upgrade() {
                    on_notify(entity, cx)
                } else {
                    false
                }
            }),
        )
    }

    /// Arrange for the given callback to be invoked whenever the given entity emits an event of a given type.
    /// The callback is provided a handle to the emitting entity and a reference to the emitted event.
    pub fn subscribe<T, Event>(
        &mut self,
        entity: &Entity<T>,
        mut on_event: impl FnMut(Entity<T>, &Event, &mut App) + 'static,
    ) -> Subscription
    where
        T: 'static + EventEmitter<Event>,
        Event: 'static,
    {
        self.subscribe_internal(entity, move |entity, event, cx| {
            on_event(entity, event, cx);
            true
        })
    }

    pub(crate) fn new_subscription(
        &mut self,
        key: EntityId,
        value: (TypeId, Listener),
    ) -> Subscription {
        let (subscription, activate) = self.event_listeners.insert(key, value);
        self.defer(move |_| activate());
        subscription
    }
    pub(crate) fn subscribe_internal<T, Evt>(
        &mut self,
        entity: &Entity<T>,
        mut on_event: impl FnMut(Entity<T>, &Evt, &mut App) -> bool + 'static,
    ) -> Subscription
    where
        T: 'static + EventEmitter<Evt>,
        Evt: 'static,
    {
        let entity_id = entity.entity_id();
        let handle = entity.downgrade();
        self.new_subscription(
            entity_id,
            (
                TypeId::of::<Evt>(),
                Box::new(move |event, cx| {
                    let Some(event) = event.downcast_ref() else {
                        return false;
                    };
                    if let Some(entity) = handle.upgrade() {
                        on_event(entity, event, cx)
                    } else {
                        false
                    }
                }),
            ),
        )
    }

    /// Returns handles to all open windows in the application.
    /// Each handle could be downcast to a handle typed for the root view of that window.
    /// To find all windows of a given type, you could filter on
    pub fn windows(&self) -> Vec<AnyWindowHandle> {
        self.windows
            .keys()
            .flat_map(|window_id| self.window_handles.get(&window_id).copied())
            .collect()
    }

    /// Returns the window handles ordered by their appearance on screen, front to back.
    ///
    /// The first window in the returned list is the active/topmost window of the application.
    ///
    /// This method returns None if the platform doesn't implement the method yet.
    pub fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        self.platform.window_stack()
    }

    /// Returns a handle to the window that is currently focused at the platform level, if one exists.
    pub fn active_window(&self) -> Option<AnyWindowHandle> {
        self.platform.active_window()
    }

    /// Open or return the active workspace.
    pub fn open_workspace(&mut self) -> &mut crate::workspace::Workspace {
        self.workspace
            .get_or_insert_with(crate::workspace::Workspace::new)
    }

    /// Close the active workspace.
    pub fn close_workspace(&mut self) {
        self.workspace = None;
    }

    /// Return the active workspace, if any.
    pub fn workspace(&self) -> Option<&crate::workspace::Workspace> {
        self.workspace.as_ref()
    }

    /// Register a command in the global command registry.
    pub fn register_command(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        handler: impl Fn() + Send + Sync + 'static,
    ) {
        self.command_registry.register_action(id, name, handler);
    }

    /// Schedule a background job via the job scheduler.
    pub fn schedule_job<Job>(&self, job: Job) -> anyhow::Result<String>
    where
        Job: crate::background_jobs::BackgroundJob,
    {
        self.background_jobs.schedule(job)
    }

    fn handle_open_urls(&mut self, urls: Vec<String>) {
        let requests = urls
            .iter()
            .cloned()
            .map(OpenRequest::parse)
            .collect::<Vec<_>>();

        let mut batch_observers = mem::take(&mut self.open_urls_observers);
        for observer in batch_observers.iter_mut() {
            observer(urls.clone());
        }
        let mut newly_registered_batch_observers = mem::take(&mut self.open_urls_observers);
        batch_observers.append(&mut newly_registered_batch_observers);
        self.open_urls_observers = batch_observers;

        let mut request_batch_observers = mem::take(&mut self.open_requests_observers);
        for observer in request_batch_observers.iter_mut() {
            observer(requests.clone());
        }
        let mut newly_registered_request_batch_observers =
            mem::take(&mut self.open_requests_observers);
        request_batch_observers.append(&mut newly_registered_request_batch_observers);
        self.open_requests_observers = request_batch_observers;

        for (url, request) in urls.into_iter().zip(requests.into_iter()) {
            let _ = self.dispatch_deep_link(&url);

            let scheme = url_scheme(&url).to_string();
            let mut deep_link_observers =
                self.deep_link_observers.remove(&scheme).unwrap_or_default();
            for observer in deep_link_observers.iter_mut() {
                observer(url.clone(), self);
            }
            let mut newly_registered_deep_link_observers =
                self.deep_link_observers.remove(&scheme).unwrap_or_default();
            deep_link_observers.append(&mut newly_registered_deep_link_observers);
            if !deep_link_observers.is_empty() {
                self.deep_link_observers.insert(scheme, deep_link_observers);
            }

            let mut open_url_observers = mem::take(&mut self.open_url_observers);
            for observer in open_url_observers.iter_mut() {
                observer(url.clone(), self);
            }
            let mut newly_registered_open_url_observers = mem::take(&mut self.open_url_observers);
            open_url_observers.append(&mut newly_registered_open_url_observers);
            self.open_url_observers = open_url_observers;

            let mut open_request_observers = mem::take(&mut self.open_request_observers);
            for observer in open_request_observers.iter_mut() {
                observer(request.clone(), self);
            }
            let mut newly_registered_open_request_observers =
                mem::take(&mut self.open_request_observers);
            open_request_observers.append(&mut newly_registered_open_request_observers);
            self.open_request_observers = open_request_observers;
        }
    }

    /// Opens a new window with the given option and the root view returned by the given function.
    /// The function is invoked with a `Window`, which can be used to interact with window-specific
    /// functionality.
    pub fn open_window<V: 'static + Render>(
        &mut self,
        options: impl Into<crate::WindowOptions>,
        build_root_view: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<WindowHandle<V>> {
        let options = options.into();
        self.update(|cx| {
            let id = cx.windows.insert(None);
            let handle = WindowHandle::new(id);
            match Window::new(handle.into(), options, cx) {
                Ok(mut window) => {
                    cx.window_update_stack.push(id);
                    let root_view = build_root_view(&mut window, cx);
                    cx.window_update_stack.pop();
                    window.root.replace(root_view.into());
                    window.defer(cx, |window: &mut Window, cx| window.appearance_changed(cx));

                    // allow a window to draw at least once before returning
                    // this didn't cause any issues on non windows platforms as it seems we always won the race to on_request_frame
                    // on windows we quite frequently lose the race and return a window that has never rendered, which leads to a crash
                    // where DispatchTree::root_node_id asserts on empty nodes
                    let clear = window.draw(cx);
                    clear.clear();

                    cx.window_handles.insert(id, window.handle);
                    cx.windows
                        .get_mut(id)
                        .context("window slot missing after opening window")?
                        .replace(window);
                    Ok(handle)
                }
                Err(e) => {
                    cx.windows.remove(id);
                    Err(e)
                }
            }
        })
    }

    /// Instructs the platform to activate the application by bringing it to the foreground.
    pub fn activate(&self, ignoring_other_apps: bool) {
        self.platform.activate(ignoring_other_apps);
    }

    /// Hide the application at the platform level.
    pub fn hide(&self) {
        self.platform.hide();
    }

    /// Hide other applications at the platform level.
    pub fn hide_other_apps(&self) {
        self.platform.hide_other_apps();
    }

    /// Unhide other applications at the platform level.
    pub fn unhide_other_apps(&self) {
        self.platform.unhide_other_apps();
    }

    /// Set the system tray icon.
    pub fn set_tray_icon(&self, icon: Option<&[u8]>) {
        self.platform.set_tray_icon(icon);
    }

    /// Set the system tray menu items.
    pub fn set_tray_menu(&self, menu: impl Into<Vec<TrayMenuItem>>) {
        self.platform.set_tray_menu(menu.into());
    }

    /// Validate and set the system tray menu items.
    pub fn set_tray_menu_checked(&self, menu: TrayMenuBuilder) -> Result<Vec<TrayMenuItem>> {
        let items = menu.build()?;
        self.platform.set_tray_menu(items.clone());
        Ok(items)
    }

    /// Validate and install a complete tray/background-app configuration.
    pub fn configure_tray_app_checked(
        &mut self,
        tray_app: TrayAppBuilder,
    ) -> Result<TrayAppConfig> {
        let config = tray_app.build_checked()?;
        self.platform.set_tray_menu(config.menu.clone());
        self.platform
            .set_tray_tooltip(config.tooltip.as_deref().unwrap_or_default());
        self.platform.set_tray_panel_mode(config.panel_mode);
        self.platform
            .set_keep_alive_without_windows(config.keep_alive_without_windows);
        self.keep_alive_without_windows = config.keep_alive_without_windows;
        Ok(config)
    }

    /// Set the system tray tooltip.
    pub fn set_tray_tooltip(&self, tooltip: &str) {
        self.platform.set_tray_tooltip(tooltip);
    }

    /// Validate and set the system tray tooltip.
    pub fn set_tray_tooltip_checked(&self, tooltip: TrayTooltipBuilder) -> Result<()> {
        let tooltip = tooltip.build_checked()?;
        self.set_tray_tooltip(tooltip.as_deref().unwrap_or_default());
        Ok(())
    }

    /// Enable or disable tray panel mode.
    /// When enabled, clicking the tray icon fires `TrayIconEvent::LeftClick` instead of showing the NSMenu.
    pub fn set_tray_panel_mode(&self, enabled: bool) {
        self.platform.set_tray_panel_mode(enabled);
    }

    /// Get the screen bounds of the tray icon, useful for positioning a panel below it.
    pub fn tray_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        self.platform.get_tray_icon_bounds()
    }

    /// Register a callback for system tray icon events.
    pub fn on_tray_icon_event(&self, mut callback: impl FnMut(TrayIconEvent, &mut App) + 'static) {
        let this = self.this.clone();
        self.platform.on_tray_icon_event(Box::new(move |event| {
            if let Some(app) = this.upgrade() {
                callback(event, &mut app.borrow_mut());
            }
        }));
    }

    /// Register a callback for when a tray menu item is clicked.
    pub fn on_tray_menu_action(&self, mut callback: impl FnMut(SharedString, &mut App) + 'static) {
        let this = self.this.clone();
        self.platform.on_tray_menu_action(Box::new(move |id| {
            if let Some(app) = this.upgrade() {
                callback(id, &mut app.borrow_mut());
            }
        }));
    }

    /// Register a global hotkey with the given ID and keystroke.
    pub fn register_global_hotkey(&self, id: u32, keystroke: &Keystroke) -> Result<()> {
        self.platform.register_global_hotkey(id, keystroke)
    }

    /// Register several global hotkeys from a builder-friendly set.
    pub fn register_global_hotkeys(&self, hotkeys: impl Into<GlobalHotkeySet>) -> Result<()> {
        for hotkey in hotkeys.into().into_hotkeys() {
            self.register_global_hotkey(hotkey.id(), hotkey.keystroke())?;
        }

        Ok(())
    }

    /// Validate and register several global hotkeys from a builder-friendly set.
    pub fn register_global_hotkeys_checked(
        &self,
        hotkeys: GlobalHotkeyBuilder,
    ) -> Result<GlobalHotkeySet> {
        let hotkeys = hotkeys.build_checked()?;
        hotkeys.validate()?;

        for hotkey in hotkeys.hotkeys() {
            self.register_global_hotkey(hotkey.id(), hotkey.keystroke())?;
        }

        Ok(hotkeys)
    }

    /// Unregister a previously registered global hotkey.
    pub fn unregister_global_hotkey(&self, id: u32) {
        self.platform.unregister_global_hotkey(id);
    }

    /// Register a callback for global hotkey events.
    pub fn on_global_hotkey(&self, callback: impl FnMut(u32) + 'static) {
        self.platform.on_global_hotkey(Box::new(callback));
    }

    /// Register a callback for global hotkey key-up (release) events.
    pub fn on_global_hotkey_up(&self, callback: impl FnMut(u32) + 'static) {
        self.platform.on_global_hotkey_up(Box::new(callback));
    }

    /// Get information about the currently focused window from any application.
    pub fn focused_window_info(&self) -> Option<FocusedWindowInfo> {
        self.platform.focused_window_info()
    }

    /// Get focused-window information when it satisfies a checked query.
    pub fn focused_window_info_checked(
        &self,
        query: impl Into<FocusedWindowQueryBuilder>,
    ) -> Result<Option<FocusedWindowInfo>> {
        let query = query.into().build_checked()?;
        let Some(info) = self.focused_window_info() else {
            return Ok(None);
        };
        Ok(info.matches_query(&query).then_some(info))
    }

    /// Check accessibility permission status.
    pub fn accessibility_status(&self) -> PermissionStatus {
        self.platform.accessibility_status()
    }

    /// Request accessibility permission from the user.
    pub fn request_accessibility_permission(&self) {
        self.platform.request_accessibility_permission();
    }

    /// Check microphone permission status.
    pub fn microphone_status(&self) -> PermissionStatus {
        self.platform.microphone_status()
    }

    /// Request microphone permission from the user.
    pub fn request_microphone_permission(&self, callback: impl FnOnce(bool) + 'static) {
        self.platform
            .request_microphone_permission(Box::new(callback));
    }

    /// Check camera permission status.
    pub fn camera_status(&self) -> PermissionStatus {
        self.platform.camera_status()
    }

    /// Request camera permission from the user.
    pub fn request_camera_permission(&self, callback: impl FnOnce(bool) + 'static) {
        self.platform.request_camera_permission(Box::new(callback));
    }

    /// Check and request one or more common OS permissions from a builder.
    pub fn request_permissions(
        &self,
        permissions: PermissionRequestBuilder,
    ) -> Result<PermissionRequestResult> {
        permissions.validate()?;
        let (
            request_accessibility,
            request_microphone,
            request_camera,
            microphone_callback,
            camera_callback,
        ) = permissions.into_parts();
        let mut result = PermissionRequestResult::default();

        if request_accessibility {
            let status = self.accessibility_status();
            result.accessibility = Some(status);
            if matches!(status, PermissionStatus::NotDetermined) {
                self.request_accessibility_permission();
                result.requested_accessibility = true;
            }
        }

        if request_microphone {
            let status = self.microphone_status();
            result.microphone = Some(status);
            if matches!(status, PermissionStatus::NotDetermined) {
                self.platform.request_microphone_permission(
                    microphone_callback.unwrap_or_else(|| Box::new(|_| {})),
                );
                result.requested_microphone = true;
            }
        }

        if request_camera {
            let status = self.camera_status();
            result.camera = Some(status);
            if matches!(status, PermissionStatus::NotDetermined) {
                self.platform
                    .request_camera_permission(camera_callback.unwrap_or_else(|| Box::new(|_| {})));
                result.requested_camera = true;
            }
        }

        Ok(result)
    }

    /// Set whether the application should auto-launch at login.
    pub fn set_auto_launch(&self, app_id: &str, enabled: bool) -> Result<()> {
        self.platform.set_auto_launch(app_id, enabled)
    }

    /// Check whether the application is set to auto-launch at login.
    pub fn is_auto_launch_enabled(&self, app_id: &str) -> bool {
        self.platform.is_auto_launch_enabled(app_id)
    }

    /// Configure launch-at-login with a validated builder and return the
    /// resulting platform state.
    pub fn configure_auto_launch(
        &self,
        auto_launch: AutoLaunchBuilder,
    ) -> Result<AutoLaunchStatus> {
        let (app_id, enabled) = auto_launch.build_checked()?;
        self.set_auto_launch(&app_id, enabled)?;
        Ok(AutoLaunchStatus {
            enabled: self.is_auto_launch_enabled(&app_id),
            app_id,
        })
    }

    /// Show an OS notification.
    pub fn show_notification(&self, title: &str, body: &str) -> Result<()> {
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::Notification)
        {
            PermissionResult::Granted => self.platform.show_notification(title, body),
            PermissionResult::Denied => Err(anyhow!("capability denied: Notification")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: Notification")),
        }
    }

    /// Validate and show a plain OS notification.
    pub fn show_notification_checked(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<()> {
        self.show_desktop_notification(NotificationBuilder::new(title, body))
    }

    /// Show an OS notification using the builder-friendly notification API.
    ///
    /// Use [`Self::show_desktop_notification_with_actions`] when the notification
    /// includes action buttons.
    pub fn show_desktop_notification(&self, notification: NotificationBuilder) -> Result<()> {
        notification.validate()?;

        if notification.has_actions() {
            return Err(anyhow!(
                "notification has action buttons; use show_desktop_notification_with_actions to handle them"
            ));
        }

        let (title, body, _) = notification.into_parts();
        self.show_notification(&title, &body)
    }

    /// Show an OS notification with action buttons.
    pub fn show_notification_with_actions(
        &self,
        title: &str,
        body: &str,
        actions: &[NotificationAction],
        callback: impl FnMut(String) + 'static,
    ) -> Result<()> {
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::Notification)
        {
            PermissionResult::Granted => self.platform.show_notification_with_actions(
                title,
                body,
                actions,
                Box::new(callback),
            ),
            PermissionResult::Denied => Err(anyhow!("capability denied: Notification")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: Notification")),
        }
    }

    /// Show an OS notification using the builder-friendly action API.
    pub fn show_desktop_notification_with_actions(
        &self,
        notification: NotificationBuilder,
        callback: impl FnMut(String) + 'static,
    ) -> Result<()> {
        notification.validate()?;

        let (title, body, actions) = notification.into_parts();
        if actions.is_empty() {
            return self.show_notification(&title, &body);
        }

        self.show_notification_with_actions(&title, &body, &actions, callback)
    }

    /// Set whether the application should stay alive when all windows are closed.
    pub fn set_keep_alive_without_windows(&mut self, keep_alive: bool) {
        self.platform.set_keep_alive_without_windows(keep_alive);
        self.keep_alive_without_windows = keep_alive;
    }

    /// Return the configured timeout for `on_app_quit` cleanup futures.
    pub fn quit_cleanup_timeout(&self) -> Duration {
        self.shutdown_timeout
    }

    /// Snapshot app runtime state for readiness checks, diagnostics, and agents.
    pub fn runtime_snapshot(&self) -> AppRuntimeSnapshot {
        AppRuntimeSnapshot {
            process_id: self.current_process_id,
            uptime: self.started_at.elapsed(),
            window_count: self.windows.len(),
            keep_alive_without_windows: self.keep_alive_without_windows,
            quit_cleanup_timeout: self.shutdown_timeout,
            quitting: self.quitting,
            network_status: self.network_status(),
            power: self.system_power_snapshot(),
            theme: self.native_theme_snapshot(),
        }
    }

    /// Build a checked app-window visual capture request for tests, diagnostics, or agents.
    pub fn app_window_capture_request_checked(
        &self,
        request: AppWindowCaptureRequestBuilder,
    ) -> Result<AppWindowCaptureRequest> {
        request.build_checked()
    }

    /// Validate and apply app lifecycle policy.
    pub fn configure_lifecycle_policy_checked(
        &mut self,
        policy: AppLifecyclePolicyBuilder,
    ) -> Result<AppLifecyclePolicy> {
        let policy = policy.build_checked()?;
        self.shutdown_timeout = policy.quit_cleanup_timeout;
        self.keep_alive_without_windows = policy.keep_alive_without_windows();
        self.platform
            .set_keep_alive_without_windows(policy.keep_alive_without_windows());
        Ok(policy)
    }

    /// Validate and perform an app-level activation or lifecycle command.
    pub fn perform_lifecycle_command_checked(
        &mut self,
        command: AppLifecycleCommand,
    ) -> Result<AppLifecycleCommand> {
        command.validate()?;
        match command.kind() {
            AppLifecycleCommandKind::Activate {
                ignoring_other_apps,
            } => self.activate(ignoring_other_apps),
            AppLifecycleCommandKind::Hide => self.hide(),
            AppLifecycleCommandKind::HideOtherApps => self.hide_other_apps(),
            AppLifecycleCommandKind::UnhideOtherApps => self.unhide_other_apps(),
            AppLifecycleCommandKind::Quit => self.quit(),
            AppLifecycleCommandKind::Restart => self.restart(),
        }
        Ok(command)
    }

    /// Register a callback for system power events (sleep, wake, shutdown).
    pub fn on_system_power_event(
        &mut self,
        mut callback: impl FnMut(SystemPowerEvent, &mut App) + 'static,
    ) {
        self.system_power_handler = Some(Box::new(move |event, cx| callback(event, cx)));
    }

    /// Capture the current power, idle, and reduce-motion state.
    pub fn system_power_snapshot(&self) -> SystemPowerSnapshot {
        SystemPowerSnapshot {
            power_mode: self.power_mode(),
            reduce_motion: self.reduce_motion(),
            idle_time: self.system_idle_time(),
        }
    }

    /// Monitor system power events with builder-friendly callbacks and an initial snapshot.
    pub fn watch_system_power(&mut self, monitor: SystemPowerMonitorBuilder) -> SystemPowerMonitor {
        let initial_snapshot = self.system_power_snapshot();
        let (
            mut on_event,
            mut on_suspend,
            mut on_resume,
            mut on_power_mode_changed,
            mut on_lock_screen,
            mut on_unlock_screen,
            mut on_shutdown,
        ) = monitor.into_parts();

        self.on_system_power_event(move |event, app| {
            let snapshot = app.system_power_snapshot();
            if let Some(callback) = on_event.as_mut() {
                callback(event, &snapshot, app);
            }

            match event {
                SystemPowerEvent::Suspend => {
                    if let Some(callback) = on_suspend.as_mut() {
                        callback(&snapshot, app);
                    }
                }
                SystemPowerEvent::Resume => {
                    if let Some(callback) = on_resume.as_mut() {
                        callback(&snapshot, app);
                    }
                }
                SystemPowerEvent::PowerModeChanged => {
                    if let Some(callback) = on_power_mode_changed.as_mut() {
                        callback(&snapshot, app);
                    }
                }
                SystemPowerEvent::LockScreen => {
                    if let Some(callback) = on_lock_screen.as_mut() {
                        callback(&snapshot, app);
                    }
                }
                SystemPowerEvent::UnlockScreen => {
                    if let Some(callback) = on_unlock_screen.as_mut() {
                        callback(&snapshot, app);
                    }
                }
                SystemPowerEvent::Shutdown => {
                    if let Some(callback) = on_shutdown.as_mut() {
                        callback(&snapshot, app);
                    }
                }
            }
        });

        SystemPowerMonitor { initial_snapshot }
    }

    /// Validate and monitor system power events with at least one callback.
    pub fn watch_system_power_checked(
        &mut self,
        monitor: SystemPowerMonitorBuilder,
    ) -> Result<SystemPowerMonitor> {
        monitor.validate()?;
        Ok(self.watch_system_power(monitor))
    }

    /// Start a power save blocker to prevent the system from sleeping or the display from dimming.
    pub fn start_power_save_blocker(&self, kind: PowerSaveBlockerKind) -> Option<u32> {
        self.platform.start_power_save_blocker(kind)
    }

    /// Start a power-save blocker from a builder and return a typed handle.
    pub fn start_power_save_blocker_with(
        &self,
        blocker: impl Into<PowerSaveBlockerBuilder>,
    ) -> Option<PowerSaveBlockerHandle> {
        let (kind, reason) = blocker.into().into_parts();
        self.start_power_save_blocker(kind)
            .map(|id| PowerSaveBlockerHandle { id, kind, reason })
    }

    /// Validate and start a power-save blocker from a builder.
    pub fn start_power_save_blocker_checked(
        &self,
        blocker: PowerSaveBlockerBuilder,
    ) -> Result<Option<PowerSaveBlockerHandle>> {
        let (kind, reason) = blocker.build_checked()?;
        Ok(self
            .start_power_save_blocker(kind)
            .map(|id| PowerSaveBlockerHandle { id, kind, reason }))
    }

    /// Stop a previously started power save blocker by its ID.
    pub fn stop_power_save_blocker(&self, id: u32) {
        self.platform.stop_power_save_blocker(id);
    }

    /// Get the current system power mode.
    pub fn power_mode(&self) -> PowerMode {
        self.platform.power_mode()
    }

    /// Whether the OS "reduce motion" accessibility preference is enabled. The framework
    /// folds this into [`crate::Window::animations_enabled`]; apps should also minimize
    /// non-essential motion when this is true.
    pub fn reduce_motion(&self) -> bool {
        self.platform.should_reduce_motion()
    }

    /// Query the default GPU's current memory budget and usage, or `None` if unavailable.
    ///
    /// Backed by the native API per platform (Metal `recommendedMaxWorkingSetSize`, DXGI
    /// `QueryVideoMemoryInfo`, Vulkan `VK_EXT_memory_budget`). Lets apps monitor GPU memory
    /// pressure and shed their own caches; pair with [`crate::GpuMemoryManager`] to enforce
    /// a budget over registered GPU resources.
    pub fn gpu_memory_budget(&self) -> Option<crate::GpuMemoryBudget> {
        crate::GpuMemoryBudget::query()
    }

    /// Get the duration since the last user input event.
    pub fn system_idle_time(&self) -> Option<Duration> {
        self.platform.system_idle_time()
    }

    /// Evaluate the current system idle state against a checked policy.
    pub fn system_idle_evaluation_checked(
        &self,
        policy: impl Into<SystemIdlePolicyBuilder>,
    ) -> Result<SystemIdleEvaluation> {
        let policy = policy.into().build_checked()?;
        Ok(policy.evaluate(&self.system_power_snapshot()))
    }

    /// Get the current network connectivity status.
    pub fn network_status(&self) -> NetworkStatus {
        self.platform.network_status()
    }

    /// Register a callback for network connectivity status changes.
    pub fn on_network_status_change(
        &self,
        mut callback: impl FnMut(NetworkStatus, &mut App) + 'static,
    ) {
        let this = self.this.clone();
        self.platform
            .on_network_status_change(Box::new(move |status| {
                if let Some(app) = this.upgrade() {
                    callback(status, &mut app.borrow_mut());
                }
            }));
    }

    /// Install a builder-friendly network status monitor.
    pub fn watch_network_status(
        &self,
        monitor: NetworkStatusMonitorBuilder,
    ) -> NetworkStatusMonitor {
        let initial_status = self.network_status();
        let (mut on_change, mut on_online, mut on_offline) = monitor.into_parts();

        self.on_network_status_change(move |status, app| {
            if let Some(callback) = on_change.as_mut() {
                callback(status, app);
            }

            match status {
                NetworkStatus::Online => {
                    if let Some(callback) = on_online.as_mut() {
                        callback(app);
                    }
                }
                NetworkStatus::Offline => {
                    if let Some(callback) = on_offline.as_mut() {
                        callback(app);
                    }
                }
            }
        });

        NetworkStatusMonitor { initial_status }
    }

    /// Validate and install a network status monitor with at least one callback.
    pub fn watch_network_status_checked(
        &self,
        monitor: NetworkStatusMonitorBuilder,
    ) -> Result<NetworkStatusMonitor> {
        monitor.validate()?;
        Ok(self.watch_network_status(monitor))
    }

    /// Register a callback for media key events (play, pause, next, previous).
    pub fn on_media_key_event(&self, mut callback: impl FnMut(MediaKeyEvent, &mut App) + 'static) {
        let this = self.this.clone();
        self.platform.on_media_key_event(Box::new(move |event| {
            if let Some(app) = this.upgrade() {
                callback(event, &mut app.borrow_mut());
            }
        }));
    }

    /// Request the user's attention by bouncing the dock icon or flashing the taskbar.
    pub fn request_user_attention(&self, attention_type: AttentionType) {
        self.platform.request_user_attention(attention_type);
    }

    /// Request user attention from a builder and return a typed cancellation handle.
    pub fn request_user_attention_with(
        &self,
        attention: impl Into<UserAttentionBuilder>,
    ) -> UserAttentionRequest {
        let (attention_type, reason) = attention.into().into_parts();
        self.request_user_attention(attention_type);
        UserAttentionRequest {
            attention_type,
            reason,
        }
    }

    /// Validate and request user attention, returning a typed cancellation handle.
    pub fn request_user_attention_checked(
        &self,
        attention: UserAttentionBuilder,
    ) -> Result<UserAttentionRequest> {
        let (attention_type, reason) = attention.build_checked()?;
        self.request_user_attention(attention_type);
        Ok(UserAttentionRequest {
            attention_type,
            reason,
        })
    }

    /// Cancel a previous user attention request.
    pub fn cancel_user_attention(&self) {
        self.platform.cancel_user_attention();
    }

    /// Set the dock badge label (macOS) or taskbar overlay text.
    pub fn set_dock_badge(&self, label: Option<&str>) {
        self.platform.set_dock_badge(label);
    }

    /// Validate and set the dock badge label (macOS) or taskbar overlay text.
    pub fn set_dock_badge_checked(&self, badge: DockBadgeBuilder) -> Result<()> {
        let label = badge.build_checked()?;
        self.set_dock_badge(label.as_deref());
        Ok(())
    }

    /// Show a context menu at the given screen position with the specified menu items.
    pub fn show_context_menu(
        &self,
        position: Point<Pixels>,
        items: impl Into<Vec<TrayMenuItem>>,
        mut callback: impl FnMut(SharedString, &mut App) + 'static,
    ) {
        let this = self.this.clone();
        self.platform.show_context_menu(
            position,
            items.into(),
            Box::new(move |id| {
                if let Some(app) = this.upgrade() {
                    callback(id, &mut app.borrow_mut());
                }
            }),
        );
    }

    /// Validate and show a native context menu.
    pub fn show_context_menu_checked(
        &self,
        position: Point<Pixels>,
        items: NativeContextMenuBuilder,
        callback: impl FnMut(SharedString, &mut App) + 'static,
    ) -> Result<Vec<TrayMenuItem>> {
        let items = items.build()?;
        self.show_context_menu(position, items.clone(), callback);
        Ok(items)
    }

    /// Show a native dialog with the given options, returning the index of the clicked button.
    pub fn show_dialog(&self, options: DialogOptions) -> oneshot::Receiver<usize> {
        self.platform.show_dialog(options)
    }

    /// Show a native message dialog using the builder-friendly API.
    pub fn show_message_dialog(
        &self,
        dialog: MessageDialogBuilder,
    ) -> Result<oneshot::Receiver<usize>> {
        dialog.validate()?;
        Ok(self.show_dialog(dialog.into_options()))
    }

    /// Validate app metadata and show an informational About dialog.
    pub fn show_about_dialog_checked(
        &self,
        metadata: AppMetadataBuilder,
    ) -> Result<oneshot::Receiver<usize>> {
        self.show_message_dialog(metadata.build_checked()?.about_dialog())
    }

    /// Build checked update state for menus, notifications, settings, and agents.
    pub fn app_update_state_checked(
        &self,
        update: AppUpdateStateBuilder,
    ) -> Result<AppUpdateState> {
        update.build_checked()
    }

    /// Apply checked update offer policy to one discovered release.
    pub fn app_update_offer_checked(
        &self,
        policy: AppUpdateOfferPolicyBuilder,
        release: AppUpdateReleaseBuilder,
    ) -> Result<AppUpdateOfferDecision> {
        let policy = policy.build_checked()?;
        let release = release.build_checked()?;
        Ok(policy.evaluate_release(&release))
    }

    /// Get operating system information (name, version, architecture).
    pub fn os_info(&self) -> OsInfo {
        self.platform.os_info()
    }

    /// Check whether biometric authentication (Touch ID, Windows Hello) is available.
    pub fn biometric_status(&self) -> BiometricStatus {
        self.platform.biometric_status()
    }

    /// Authenticate the user via biometrics with the given reason string.
    pub fn authenticate_biometric(
        &self,
        reason: &str,
        callback: impl FnOnce(bool) + Send + 'static,
    ) {
        self.platform
            .authenticate_biometric(reason, Box::new(callback));
    }

    /// Authenticate the user via biometrics using a validated prompt builder.
    pub fn authenticate_biometric_with(
        &self,
        prompt: BiometricAuthBuilder,
        callback: impl FnOnce(bool) + Send + 'static,
    ) -> Result<BiometricAuthRequest> {
        let (reason, require_available) = prompt.build_checked()?;
        let status = self.biometric_status();
        let prompted = matches!(status, BiometricStatus::Available(_)) || !require_available;

        if prompted {
            self.authenticate_biometric(&reason, callback);
        }

        Ok(BiometricAuthRequest {
            status,
            prompted,
            reason,
        })
    }

    /// Install a panic hook that captures crash reports with backtraces and OS info.
    pub fn set_crash_handler(
        &self,
        app_version: Option<String>,
        handler: impl Fn(CrashReport) + Send + Sync + 'static,
    ) {
        let os_info = self.platform.os_info();
        let handler = std::sync::Arc::new(handler);
        std::panic::set_hook(Box::new(move |panic_info| {
            let message = if let Some(msg) = panic_info.payload().downcast_ref::<&str>() {
                msg.to_string()
            } else if let Some(msg) = panic_info.payload().downcast_ref::<String>() {
                msg.clone()
            } else {
                "Unknown panic".to_string()
            };

            let location = panic_info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                .unwrap_or_default();

            let full_message = if location.is_empty() {
                message
            } else {
                format!("{message} at {location}")
            };

            let backtrace = std::backtrace::Backtrace::force_capture().to_string();

            let report = CrashReport {
                message: full_message,
                backtrace,
                os_info: os_info.clone(),
                app_version: app_version.clone(),
            };

            handler(report);
        }));
    }

    /// Build and install a checked crash reporter panic hook.
    pub fn install_crash_reporter_checked(
        &self,
        reporter: CrashReporterBuilder,
    ) -> Result<CrashReporter> {
        reporter.install_hook_checked()
    }

    /// Return the app-owned custom protocol schemes registered on this app.
    pub fn custom_protocol_schemes(&self) -> Vec<String> {
        let mut schemes = self
            .custom_protocol_handlers
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        schemes.sort();
        schemes
    }

    /// Return true when this app has a handler for a custom protocol scheme.
    pub fn has_custom_protocol(&self, scheme: &str) -> bool {
        self.custom_protocol_handlers.contains_key(scheme)
    }

    /// Handle an app-owned custom protocol URL through the registered route.
    ///
    /// Returns `Ok(None)` when no route exists for the URL's scheme.
    pub fn handle_custom_protocol_url(
        &mut self,
        url: impl Into<String>,
    ) -> Result<Option<CustomProtocolResponse>> {
        let request = CustomProtocolRequest::parse(url)?;
        let scheme = request.scheme().to_string();
        let Some(mut handler) = self.custom_protocol_handlers.remove(&scheme) else {
            return Ok(None);
        };

        let response = handler(request, self);
        let mut newly_registered = self.custom_protocol_handlers.remove(&scheme);
        if let Some(new_handler) = newly_registered.take() {
            self.custom_protocol_handlers.insert(scheme, new_handler);
        } else {
            self.custom_protocol_handlers.insert(scheme, handler);
        }

        let response = response?;
        response.validate()?;
        Ok(Some(response))
    }

    /// Return current platform support for native share sheets.
    #[cfg(feature = "share")]
    pub fn share_support(&self) -> kael_share::PlatformShareSupport {
        kael_share::ShareSheet::default().platform_support()
    }

    /// Validate and launch a native share sheet.
    #[cfg(feature = "share")]
    pub async fn show_share_sheet_checked(
        &self,
        sheet: kael_share::ShareSheet,
    ) -> Result<kael_share::ShareResult> {
        sheet.validate()?;
        sheet.show().await
    }

    /// Validate and launch a native share sheet from a checked builder.
    #[cfg(feature = "share")]
    pub async fn show_share_sheet(
        &self,
        sheet: kael_share::ShareSheetBuilder,
    ) -> Result<kael_share::ShareResult> {
        self.show_share_sheet_checked(sheet.build_checked()?).await
    }

    /// Returns the list of currently active displays.
    pub fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.platform.displays()
    }

    /// Returns the primary display that will be used for new windows.
    pub fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.platform.primary_display()
    }

    /// Returns the current global cursor (mouse) position in screen coordinates.
    pub fn cursor_position(&self) -> Option<Point<Pixels>> {
        self.platform.cursor_position()
    }

    /// Query active displays using a checked Electron `screen`-style selector.
    pub fn query_displays_checked(&self, query: DisplayQueryBuilder) -> Result<DisplayQueryResult> {
        query.validate()?;

        let displays = self.displays();
        let primary_id = self.primary_display().map(|display| display.id());
        let cursor_position = self.cursor_position();
        let snapshots = displays
            .iter()
            .map(|display| display_snapshot(display.as_ref(), primary_id, cursor_position))
            .collect::<Vec<_>>();

        let mut matches = match query.target {
            DisplayQueryTarget::All => snapshots.clone(),
            DisplayQueryTarget::Primary => primary_id
                .and_then(|id| snapshots.iter().find(|display| display.id == id).cloned())
                .into_iter()
                .collect(),
            DisplayQueryTarget::Cursor => cursor_position
                .and_then(|position| {
                    snapshots
                        .iter()
                        .find(|display| display.bounds.contains(&position))
                        .cloned()
                })
                .into_iter()
                .collect(),
            DisplayQueryTarget::Display(id) => snapshots
                .iter()
                .filter(|display| display.id == id)
                .cloned()
                .collect(),
        };

        if matches.is_empty()
            && query.fallback_to_primary
            && !matches!(
                query.target,
                DisplayQueryTarget::All | DisplayQueryTarget::Primary
            )
            && let Some(primary_id) = primary_id
            && let Some(primary) = snapshots
                .iter()
                .find(|display| display.id == primary_id)
                .cloned()
        {
            matches.push(primary);
        }

        anyhow::ensure!(
            !query.require_match || !matches.is_empty(),
            "display query did not match any active displays"
        );

        Ok(DisplayQueryResult {
            target: query.target,
            displays: matches,
            cursor_position,
        })
    }

    /// Compute window bounds from a desired size and a semantic position.
    pub fn compute_window_bounds(
        &self,
        size: Size<Pixels>,
        position: &WindowPosition,
    ) -> Bounds<Pixels> {
        let displays = self.platform.displays();
        let primary = self.platform.primary_display();
        crate::platform::window_positioner::compute_window_bounds(
            size,
            position,
            &displays,
            primary.as_ref(),
        )
    }

    /// Resolve a builder-friendly placement into concrete screen bounds.
    pub fn resolve_window_placement(
        &self,
        placement: WindowPlacementBuilder,
    ) -> Result<WindowPlacement> {
        placement.validate()?;
        let bounds = self.compute_window_bounds(placement.size, &placement.position);
        let center = bounds.center();
        let display_id = self
            .displays()
            .iter()
            .find(|display| display.bounds().contains(&center))
            .map(|display| display.id());

        Ok(WindowPlacement {
            size: placement.size,
            position: placement.position,
            bounds,
            display_id,
        })
    }

    /// Returns whether `screen_capture_sources` may work.
    pub fn is_screen_capture_supported(&self) -> bool {
        self.platform.is_screen_capture_supported()
    }

    /// Returns a list of available screen capture sources.
    pub fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
        self.platform.screen_capture_sources()
    }

    /// Create a capture manager with platform backends and app permission context wired in.
    pub fn capture_manager(&self) -> CaptureManager {
        let mut manager = default_capture_manager();
        manager.set_permission_broker(self.permission_broker.clone());
        manager.set_process_id(self.current_process_id);
        manager
    }

    /// Returns the display with the given ID, if one exists.
    pub fn find_display(&self, id: DisplayId) -> Option<Rc<dyn PlatformDisplay>> {
        self.displays()
            .iter()
            .find(|display| display.id() == id)
            .cloned()
    }

    /// Returns the appearance of the application's windows.
    pub fn window_appearance(&self) -> WindowAppearance {
        self.platform.window_appearance()
    }

    /// Snapshot native theme and accessibility signals for UI decisions.
    pub fn native_theme_snapshot(&self) -> NativeThemeSnapshot {
        NativeThemeSnapshot::new(
            self.window_appearance(),
            self.reduce_motion(),
            self.power_mode(),
        )
    }

    /// Writes data to the primary selection buffer.
    /// Only available on Linux.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub fn write_to_primary(&self, item: ClipboardItem) {
        self.platform.write_to_primary(item)
    }

    /// Writes data to the platform clipboard.
    pub fn write_to_clipboard(&self, item: ClipboardItem) {
        self.platform.write_to_clipboard(item)
    }

    /// Writes plain text to the platform clipboard.
    pub fn write_clipboard_text(&self, text: impl Into<String>) {
        self.write_to_clipboard(ClipboardItem::new_string(text.into()));
    }

    /// Validates and writes plain text to the platform clipboard.
    pub fn write_clipboard_text_checked(&self, text: impl Into<String>) -> Result<()> {
        self.write_clipboard_item(ClipboardItem::builder().text(text))
    }

    /// Writes HTML plus a plain-text fallback to the platform clipboard.
    pub fn write_clipboard_html(
        &self,
        plain_text: impl Into<String>,
        html: impl Into<String>,
    ) -> Result<()> {
        self.write_clipboard_item(ClipboardItem::builder().html(plain_text, html)?)
    }

    /// Build, validate, and write a rich clipboard item.
    pub fn write_clipboard_item(&self, item: ClipboardItemBuilder) -> Result<()> {
        self.write_to_clipboard(item.build()?);
        Ok(())
    }

    /// Reads data from the primary selection buffer.
    /// Only available on Linux.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub fn read_from_primary(&self) -> Result<Option<ClipboardItem>> {
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::ClipboardRead)
        {
            PermissionResult::Granted => Ok(self.platform.read_from_primary()),
            PermissionResult::Denied => Err(anyhow!("capability denied: ClipboardRead")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: ClipboardRead")),
        }
    }

    /// Reads data from the platform clipboard.
    pub fn read_from_clipboard(&self) -> Result<Option<ClipboardItem>> {
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::ClipboardRead)
        {
            PermissionResult::Granted => Ok(self.platform.read_from_clipboard()),
            PermissionResult::Denied => Err(anyhow!("capability denied: ClipboardRead")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: ClipboardRead")),
        }
    }

    /// Reads plain text from the platform clipboard.
    pub fn read_clipboard_text(&self) -> Result<Option<String>> {
        self.read_from_clipboard()
            .map(|item| item.and_then(|item| item.text()))
    }

    /// Writes credentials to the platform keychain.
    pub fn write_credentials(
        &self,
        url: &str,
        username: &str,
        password: &[u8],
    ) -> Task<Result<()>> {
        self.platform.write_credentials(url, username, password)
    }

    /// Reads credentials from the platform keychain.
    pub fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        self.platform.read_credentials(url)
    }

    /// Deletes credentials from the platform keychain.
    pub fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        self.platform.delete_credentials(url)
    }

    /// Write a validated credential entry to the platform keychain.
    pub fn write_secure_credential(
        &self,
        credential: CredentialBuilder,
    ) -> Result<Task<Result<()>>> {
        let credential = credential.build()?;
        let (service, username, secret) = credential.into_parts();
        Ok(self.write_credentials(&service, &username, &secret))
    }

    /// Read a credential entry from the platform keychain.
    pub fn read_secure_credential(
        &self,
        service: impl AsRef<str>,
    ) -> Task<Result<Option<StoredCredential>>> {
        let task = self.read_credentials(service.as_ref());
        self.background_executor.spawn(async move {
            Ok(task
                .await?
                .map(|(username, secret)| StoredCredential::new(username, secret)))
        })
    }

    /// Read a validated credential entry from the platform keychain.
    pub fn read_secure_credential_checked(
        &self,
        service: CredentialServiceBuilder,
    ) -> Result<Task<Result<Option<StoredCredential>>>> {
        let service = service.build()?;
        Ok(self.read_secure_credential(service))
    }

    /// Delete a credential entry from the platform keychain.
    pub fn delete_secure_credential(&self, service: impl AsRef<str>) -> Task<Result<()>> {
        self.delete_credentials(service.as_ref())
    }

    /// Delete a validated credential entry from the platform keychain.
    pub fn delete_secure_credential_checked(
        &self,
        service: CredentialServiceBuilder,
    ) -> Result<Task<Result<()>>> {
        let service = service.build()?;
        Ok(self.delete_secure_credential(service))
    }

    /// Directs the platform's default browser to open the given URL.
    pub fn open_url(&self, url: &str) -> Result<()> {
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::OpenExternalUrl)
        {
            PermissionResult::Granted => {
                self.platform.open_url(url);
                Ok(())
            }
            PermissionResult::Denied => Err(anyhow!("capability denied: OpenExternalUrl")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: OpenExternalUrl")),
        }
    }

    /// Opens a URL with the system default browser or registered URL handler.
    pub fn open_external_url(&self, url: impl AsRef<str>) -> Result<()> {
        ShellTarget::url(url.as_ref()).validate()?;
        self.open_url(url.as_ref())
    }

    /// Opens a file or directory with the system default application.
    pub fn open_path(&self, path: impl AsRef<Path>) -> Result<()> {
        ShellTarget::path(path.as_ref()).validate()?;
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::ShellExecute)
        {
            PermissionResult::Granted => {
                self.platform.open_with_system(path.as_ref());
                Ok(())
            }
            PermissionResult::Denied => Err(anyhow!("capability denied: ShellExecute")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: ShellExecute")),
        }
    }

    /// Reveals a file or directory in the platform file manager.
    pub fn show_item_in_folder(&self, path: impl AsRef<Path>) -> Result<()> {
        ShellTarget::reveal_path(path.as_ref()).validate()?;
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::ShellExecute)
        {
            PermissionResult::Granted => {
                self.platform.reveal_path(path.as_ref());
                Ok(())
            }
            PermissionResult::Denied => Err(anyhow!("capability denied: ShellExecute")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: ShellExecute")),
        }
    }

    /// Opens or reveals a typed platform shell target.
    pub fn open_shell_target(&self, target: ShellTarget) -> Result<()> {
        target.validate()?;
        match target {
            ShellTarget::Url(url) => self.open_external_url(url),
            ShellTarget::Path(path) => self.open_path(path),
            ShellTarget::RevealPath(path) => self.show_item_in_folder(path),
        }
    }

    /// Opens or reveals multiple typed platform shell targets.
    pub fn open_shell_targets(
        &self,
        targets: impl Into<ShellTargetsBuilder>,
    ) -> Result<Vec<ShellTarget>> {
        let targets = targets.into().build()?;
        for target in targets.iter().cloned() {
            self.open_shell_target(target)?;
        }
        Ok(targets)
    }

    /// Validate a platform trash/recycle request before handing it to shell integration.
    pub fn trash_request_checked(&self, request: TrashRequestBuilder) -> Result<TrashRequest> {
        let request = request.build_checked()?;
        match self
            .permission_broker
            .check(self.current_process_id, &Capability::ShellExecute)
        {
            PermissionResult::Granted => Ok(request),
            PermissionResult::Denied => Err(anyhow!("capability denied: ShellExecute")),
            PermissionResult::Prompt => Err(anyhow!("capability prompt required: ShellExecute")),
        }
    }

    /// Registers the given URL scheme (e.g. `kael` for `kael://` urls) to be
    /// opened by the current app.
    ///
    /// On some platforms (e.g. macOS) you may be able to register URL schemes
    /// as part of app distribution, but this method exists to let you register
    /// schemes at runtime.
    pub fn register_url_scheme(&self, scheme: &str) -> Task<Result<()>> {
        self.platform.register_url_scheme(scheme)
    }

    /// Registers multiple URL schemes using the builder-friendly API.
    pub fn register_url_schemes(
        &self,
        registration: impl Into<UrlSchemeRegistrationBuilder>,
    ) -> Result<Vec<Task<Result<()>>>> {
        let schemes = registration.into().build()?;
        Ok(schemes
            .into_iter()
            .map(|scheme| self.register_url_scheme(&scheme))
            .collect())
    }

    /// Registers a runtime deep-link handler keyed by URL scheme.
    pub fn register_deep_link_handler(&mut self, handler: Box<dyn crate::DeepLinkHandler>) {
        self.deep_link_registry.register(handler);
    }

    /// Dispatches a URL through the registered runtime deep-link handlers.
    pub fn dispatch_deep_link(&self, url: &str) -> bool {
        self.deep_link_registry.handle(url)
    }

    /// Returns the full pathname of the current app bundle.
    ///
    /// Returns an error if the app is not being run from a bundle.
    pub fn app_path(&self) -> Result<PathBuf> {
        self.platform.app_path()
    }

    /// Validate native dropped paths before import, open, media, or project work.
    pub fn file_drop_intent_checked(&self, drop: FileDropIntentBuilder) -> Result<FileDropIntent> {
        drop.build_checked()
    }

    /// Validate outbound file drag/export items before starting a platform drag session.
    pub fn file_export_drag_checked(
        &self,
        export: FileExportDragIntentBuilder,
    ) -> Result<FileExportDragIntent> {
        export.build_checked()
    }

    /// Build checked file associations for packaging, installers, and agents.
    pub fn file_associations_checked(
        &self,
        associations: FileAssociationSetBuilder,
    ) -> Result<FileAssociationSet> {
        associations.build_checked()
    }

    /// Build checked default-handler intent for URL schemes and document types.
    pub fn default_handler_plan_checked(
        &self,
        plan: DefaultHandlerPlanBuilder,
    ) -> Result<DefaultHandlerPlan> {
        plan.build_checked()
    }

    /// Build checked icon asset metadata for packaging and native chrome.
    pub fn app_icons_checked(&self, icons: AppIconSetBuilder) -> Result<AppIconSet> {
        icons.build_checked()
    }

    /// Build a checked native file icon request for file explorers and recent documents.
    pub fn file_icon_request_checked(
        &self,
        request: FileIconRequestBuilder,
    ) -> Result<FileIconRequest> {
        request.build_checked()
    }

    /// Build checked privacy permission metadata for packaging.
    pub fn privacy_manifest_checked(
        &self,
        manifest: AppPrivacyManifestBuilder,
    ) -> Result<AppPrivacyManifest> {
        manifest.build_checked()
    }

    /// Build checked package metadata for bundlers, installers, and agents.
    pub fn package_manifest_checked(
        &self,
        manifest: AppPackageManifestBuilder,
    ) -> Result<AppPackageManifest> {
        manifest.build_checked()
    }

    /// Evaluate a checked package manifest for release-readiness issues.
    pub fn package_readiness_checked(
        &self,
        readiness: AppPackageReadinessBuilder,
    ) -> AppPackageReadinessReport {
        readiness.evaluate()
    }

    /// Build checked distribution targets for release scripts and agents.
    pub fn distribution_plan_checked(
        &self,
        plan: AppDistributionPlanBuilder,
    ) -> Result<AppDistributionPlan> {
        plan.build_checked()
    }

    /// Build checked signing/notarization declarations for release scripts.
    pub fn signing_plan_checked(&self, plan: AppSigningPlanBuilder) -> Result<AppSigningPlan> {
        plan.build_checked()
    }

    /// Classify app-owned paths from dialogs, drops, recent docs, or deep links.
    pub fn file_intake_plan_checked(
        &self,
        intake: FileIntakePlanBuilder,
    ) -> Result<FileIntakePlan> {
        intake.build_checked()
    }

    /// Resolves well-known app-owned paths for storage, cache, logs, and downloads.
    pub fn app_paths_checked(&self, paths: AppPathBuilder) -> Result<AppPathSet> {
        paths.build_checked()
    }

    /// Build a checked app-owned storage plan for settings, databases, caches, and logs.
    pub fn app_storage_plan_checked(&self, plan: AppStoragePlanBuilder) -> Result<AppStoragePlan> {
        plan.build_checked()
    }

    /// Returns current-process runtime metrics for diagnostics and resource budgets.
    pub fn current_process_metrics(&self) -> ProcessMetricsSnapshot {
        ProcessMetricsSnapshot {
            process_id: std::process::id(),
            executable_path: std::env::current_exe().ok(),
            current_dir: std::env::current_dir().ok(),
            uptime: self.started_at.elapsed(),
            window_count: self.windows.len(),
            memory: current_process_memory_metrics(),
        }
    }

    /// Evaluate the current app process against a checked resource budget.
    pub fn evaluate_resource_budget_checked(
        &self,
        budget: AppResourceBudgetBuilder,
    ) -> Result<AppResourceBudgetEvaluation> {
        let budget = budget.build_checked()?;
        let metrics = self.current_process_metrics();
        let runtime = self.runtime_snapshot();
        let mut issues = Vec::new();

        if budget.require_memory_metrics && !metrics.memory().is_supported() {
            issues.push(AppResourceBudgetIssue {
                kind: AppResourceBudgetIssueKind::MissingMemoryMetrics,
                message: "required memory metrics are unavailable".to_string(),
            });
        }

        if let Some(limit) = budget.max_resident_set_bytes {
            match metrics.resident_set_bytes() {
                Some(actual) if actual > limit => issues.push(AppResourceBudgetIssue {
                    kind: AppResourceBudgetIssueKind::ResidentSetExceeded,
                    message: format!("resident set {actual} bytes exceeded budget {limit} bytes"),
                }),
                None if budget.require_memory_metrics => {
                    if !issues
                        .iter()
                        .any(|issue| issue.kind == AppResourceBudgetIssueKind::MissingMemoryMetrics)
                    {
                        issues.push(AppResourceBudgetIssue {
                            kind: AppResourceBudgetIssueKind::MissingMemoryMetrics,
                            message: "resident set metrics are unavailable".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        if let Some(limit) = budget.max_virtual_memory_bytes {
            match metrics.virtual_memory_bytes() {
                Some(actual) if actual > limit => issues.push(AppResourceBudgetIssue {
                    kind: AppResourceBudgetIssueKind::VirtualMemoryExceeded,
                    message: format!("virtual memory {actual} bytes exceeded budget {limit} bytes"),
                }),
                None if budget.require_memory_metrics => {
                    if !issues
                        .iter()
                        .any(|issue| issue.kind == AppResourceBudgetIssueKind::MissingMemoryMetrics)
                    {
                        issues.push(AppResourceBudgetIssue {
                            kind: AppResourceBudgetIssueKind::MissingMemoryMetrics,
                            message: "virtual memory metrics are unavailable".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        if let Some(limit) = budget.max_windows
            && metrics.window_count() > limit
        {
            issues.push(AppResourceBudgetIssue {
                kind: AppResourceBudgetIssueKind::WindowCountExceeded,
                message: format!(
                    "window count {} exceeded budget {limit}",
                    metrics.window_count()
                ),
            });
        }

        if let Some(limit) = budget.max_uptime
            && metrics.uptime() > limit
        {
            issues.push(AppResourceBudgetIssue {
                kind: AppResourceBudgetIssueKind::UptimeExceeded,
                message: format!(
                    "uptime {}ms exceeded budget {}ms",
                    metrics.uptime().as_millis(),
                    limit.as_millis()
                ),
            });
        }

        if budget.warn_when_power_constrained && runtime.power().should_reduce_work() {
            issues.push(AppResourceBudgetIssue {
                kind: AppResourceBudgetIssueKind::PowerConstrained,
                message: "system state recommends reducing work".to_string(),
            });
        }

        Ok(AppResourceBudgetEvaluation {
            budget,
            metrics,
            runtime,
            issues,
        })
    }

    /// Returns launch context with args and basic process paths, without environment values.
    pub fn launch_context(&self) -> LaunchContextSnapshot {
        LaunchContextBuilder::new()
            .capture_checked()
            .unwrap_or_else(|_| LaunchContextSnapshot {
                process_id: std::process::id(),
                executable_path: std::env::current_exe().ok(),
                current_dir: std::env::current_dir().ok(),
                args: std::env::args_os()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .filter(|arg| !arg.contains('\0'))
                    .collect(),
                environment: Vec::new(),
                debug_build: cfg!(debug_assertions),
            })
    }

    /// Capture launch context using a validated allowlist and required-path policy.
    pub fn launch_context_checked(
        &self,
        context: LaunchContextBuilder,
    ) -> Result<LaunchContextSnapshot> {
        context.capture_checked()
    }

    /// Returns the current locale snapshot using system environment signals.
    pub fn locale_snapshot(&self) -> LocaleSnapshot {
        LocaleSnapshotBuilder::new()
            .build_checked()
            .unwrap_or_else(|_| {
                LocaleSnapshotBuilder::new()
                    .use_system_environment(false)
                    .build_checked()
                    .unwrap()
            })
    }

    /// Build a checked locale snapshot from explicit/system candidates.
    pub fn locale_snapshot_checked(&self, locale: LocaleSnapshotBuilder) -> Result<LocaleSnapshot> {
        locale.build_checked()
    }

    /// Build a checked spellcheck/grammar/autocorrect request descriptor.
    pub fn text_checking_request_checked(
        &self,
        request: TextCheckingRequestBuilder,
    ) -> Result<TextCheckingRequest> {
        request.build_checked()
    }

    /// Build a checked native geolocation request descriptor.
    pub fn location_request_checked(
        &self,
        request: LocationRequestBuilder,
    ) -> Result<LocationRequest> {
        request.build_checked()
    }

    /// Build a checked native device access request descriptor.
    pub fn device_access_request_checked(
        &self,
        request: DeviceAccessRequestBuilder,
    ) -> Result<DeviceAccessRequest> {
        request.build_checked()
    }

    /// Collect a privacy-aware support diagnostics snapshot.
    pub fn support_diagnostics_checked(
        &self,
        diagnostics: SupportDiagnosticsBuilder,
    ) -> Result<SupportDiagnosticsSnapshot> {
        diagnostics.build_checked(self)
    }

    /// On Linux, returns the name of the compositor in use.
    ///
    /// Returns an empty string on other platforms.
    pub fn compositor_name(&self) -> &'static str {
        self.platform.compositor_name()
    }

    /// Returns the file URL of the executable with the specified name in the application bundle
    pub fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        self.platform.path_for_auxiliary_executable(name)
    }

    /// Displays a platform modal for selecting paths.
    ///
    /// When one or more paths are selected, they'll be relayed asynchronously via the returned oneshot channel.
    /// If cancelled, a `None` will be relayed instead.
    /// May return an error on Linux if the file picker couldn't be opened.
    pub fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        match self.permission_broker.check(
            self.current_process_id,
            &Capability::FilesystemRead {
                scope: PathScope::UserSelected,
            },
        ) {
            PermissionResult::Granted => self.platform.prompt_for_paths(options),
            PermissionResult::Denied => {
                let (tx, rx) = oneshot::channel();
                tx.send(Err(anyhow!("capability denied: FilesystemRead")))
                    .ok();
                rx
            }
            PermissionResult::Prompt => {
                let (tx, rx) = oneshot::channel();
                tx.send(Err(anyhow!("capability prompt required: FilesystemRead")))
                    .ok();
                rx
            }
        }
    }

    /// Shows a native open dialog using the builder-friendly API.
    pub fn show_open_dialog(
        &self,
        dialog: OpenDialogBuilder,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        if let Err(error) = dialog.validate() {
            let (tx, rx) = oneshot::channel();
            tx.send(Err(error)).ok();
            return rx;
        }
        self.prompt_for_paths(dialog.into_options())
    }

    /// Displays a platform modal for selecting a new path where a file can be saved.
    ///
    /// The provided directory will be used to set the initial location.
    /// When a path is selected, it is relayed asynchronously via the returned oneshot channel.
    /// If cancelled, a `None` will be relayed instead.
    /// May return an error on Linux if the file picker couldn't be opened.
    pub fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        match self.permission_broker.check(
            self.current_process_id,
            &Capability::FilesystemWrite {
                scope: PathScope::UserSelected,
            },
        ) {
            PermissionResult::Granted => {
                self.platform.prompt_for_new_path(directory, suggested_name)
            }
            PermissionResult::Denied => {
                let (tx, rx) = oneshot::channel();
                tx.send(Err(anyhow!("capability denied: FilesystemWrite")))
                    .ok();
                rx
            }
            PermissionResult::Prompt => {
                let (tx, rx) = oneshot::channel();
                tx.send(Err(anyhow!("capability prompt required: FilesystemWrite")))
                    .ok();
                rx
            }
        }
    }

    /// Shows a native save dialog using the builder-friendly API.
    pub fn show_save_dialog(
        &self,
        dialog: SaveDialogBuilder,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        if let Err(error) = dialog.validate() {
            let (tx, rx) = oneshot::channel();
            tx.send(Err(error)).ok();
            return rx;
        }
        let (directory, suggested_name) = dialog.into_parts();
        self.prompt_for_new_path(&directory, suggested_name.as_deref())
    }

    /// Reveals the specified path at the platform level, such as in Finder on macOS.
    pub fn reveal_path(&self, path: &Path) {
        self.platform.reveal_path(path)
    }

    /// Opens the specified path with the system's default application.
    pub fn open_with_system(&self, path: &Path) {
        self.platform.open_with_system(path)
    }

    /// Returns whether the user has configured scrollbars to auto-hide at the platform level.
    pub fn should_auto_hide_scrollbars(&self) -> bool {
        self.platform.should_auto_hide_scrollbars()
    }

    /// Restarts the application.
    pub fn restart(&mut self) {
        self.restart_observers
            .clone()
            .retain(&(), |observer| observer(self));
        self.platform.restart(self.restart_path.take())
    }

    /// Sets the path to use when restarting the application.
    pub fn set_restart_path(&mut self, path: PathBuf) {
        self.restart_path = Some(path);
    }

    /// Validate and set the path to use when restarting the application.
    pub fn set_restart_path_checked(&mut self, path: RestartPathBuilder) -> Result<PathBuf> {
        let path = path.build_checked()?;
        self.set_restart_path(path.clone());
        Ok(path)
    }

    /// Returns the HTTP client for the application.
    pub fn http_client(&self) -> Arc<dyn HttpClient> {
        self.http_client.clone()
    }

    /// Sets the HTTP client for the application.
    pub fn set_http_client(&mut self, new_client: Arc<dyn HttpClient>) {
        self.http_client = new_client;
    }

    /// Returns the SVG renderer used by the application.
    pub fn svg_renderer(&self) -> SvgRenderer {
        self.svg_renderer.clone()
    }

    pub(crate) fn push_effect(&mut self, effect: Effect) {
        match &effect {
            Effect::Notify { emitter } => {
                if !self.pending_notifications.insert(*emitter) {
                    return;
                }
            }
            Effect::NotifyGlobalObservers { global_type } => {
                if !self.pending_global_notifications.insert(*global_type) {
                    return;
                }
            }
            _ => {}
        };

        self.pending_effects.push_back(effect);
    }

    /// Called at the end of [`App::update`] to complete any side effects
    /// such as notifying observers, emitting events, etc. Effects can themselves
    /// cause effects, so we continue looping until all effects are processed.
    fn flush_effects(&mut self) {
        loop {
            self.release_dropped_entities();
            self.release_dropped_focus_handles();
            if let Some(effect) = self.pending_effects.pop_front() {
                match effect {
                    Effect::Notify { emitter } => {
                        self.apply_notify_effect(emitter);
                    }

                    Effect::Emit {
                        emitter,
                        event_type,
                        event,
                    } => self.apply_emit_effect(emitter, event_type, event),

                    Effect::RefreshWindows => {
                        self.apply_refresh_effect();
                    }

                    Effect::NotifyGlobalObservers { global_type } => {
                        self.apply_notify_global_observers_effect(global_type);
                    }

                    Effect::Defer { callback } => {
                        self.apply_defer_effect(callback);
                    }
                    Effect::EntityCreated {
                        entity,
                        tid,
                        window,
                    } => {
                        self.apply_entity_created_effect(entity, tid, window);
                    }
                }
            } else {
                #[cfg(any(test, feature = "test-support"))]
                for window in self
                    .windows
                    .values()
                    .filter_map(|window| {
                        let window = window.as_ref()?;
                        window.invalidator.is_dirty().then_some(window.handle)
                    })
                    .collect::<Vec<_>>()
                {
                    if let Err(error) =
                        self.update_window(window, |_, window, cx| window.draw(cx).clear())
                    {
                        log::error!("failed to draw dirty window during effect flush: {error:#}");
                    }
                }

                if self.pending_effects.is_empty() {
                    break;
                }
            }
        }
    }

    /// Repeatedly called during `flush_effects` to release any entities whose
    /// reference count has become zero. We invoke any release observers before dropping
    /// each entity.
    fn release_dropped_entities(&mut self) {
        loop {
            let dropped = self.entities.take_dropped();
            if dropped.is_empty() {
                break;
            }

            for (entity_id, mut entity) in dropped {
                self.observers.remove(&entity_id);
                self.event_listeners.remove(&entity_id);
                for release_callback in self.release_listeners.remove(&entity_id) {
                    release_callback(entity.as_mut(), self);
                }
            }
        }
    }

    /// Repeatedly called during `flush_effects` to handle a focused handle being dropped.
    fn release_dropped_focus_handles(&mut self) {
        self.focus_handles
            .clone()
            .write()
            .retain(|handle_id, focus| {
                if focus.ref_count.load(SeqCst) == 0 {
                    for window_handle in self.windows() {
                        if let Err(error) = window_handle.update(self, |_, window, _| {
                            if window.focus == Some(handle_id) {
                                window.blur();
                            }
                        }) {
                            log::error!(
                                "failed to release dropped focus handle {:?}: {error:#}",
                                handle_id
                            );
                        }
                    }
                    false
                } else {
                    true
                }
            });
    }

    fn apply_notify_effect(&mut self, emitter: EntityId) {
        self.pending_notifications.remove(&emitter);

        self.observers
            .clone()
            .retain(&emitter, |handler| handler(self));
    }

    fn apply_emit_effect(&mut self, emitter: EntityId, event_type: TypeId, event: Box<dyn Any>) {
        self.event_listeners
            .clone()
            .retain(&emitter, |(stored_type, handler)| {
                if *stored_type == event_type {
                    handler(event.as_ref(), self)
                } else {
                    true
                }
            });
    }

    fn apply_refresh_effect(&mut self) {
        for window in self.windows.values_mut() {
            if let Some(window) = window.as_mut() {
                window.refreshing = true;
                window.invalidator.set_dirty(true);
            }
        }
    }

    fn apply_notify_global_observers_effect(&mut self, type_id: TypeId) {
        self.pending_global_notifications.remove(&type_id);
        self.global_observers
            .clone()
            .retain(&type_id, |observer| observer(self));
    }

    fn apply_defer_effect(&mut self, callback: Box<dyn FnOnce(&mut Self) + 'static>) {
        callback(self);
    }

    fn apply_entity_created_effect(
        &mut self,
        entity: AnyEntity,
        tid: TypeId,
        window: Option<WindowId>,
    ) {
        self.new_entity_observers.clone().retain(&tid, |observer| {
            if let Some(id) = window {
                if let Err(error) = self.update_window_id(id, {
                    let entity = entity.clone();
                    |_, window, cx| (observer)(entity, &mut Some(window), cx)
                }) {
                    log::error!(
                        "failed to notify new entity observer for window {id:?}: {error:#}"
                    );
                }
            } else {
                (observer)(entity.clone(), &mut None, self)
            }
            true
        });
    }

    fn update_window_id<T, F>(&mut self, id: WindowId, update: F) -> Result<T>
    where
        F: FnOnce(AnyView, &mut Window, &mut App) -> T,
    {
        self.update(|cx| {
            let mut window = cx.windows.get_mut(id)?.take()?;

            let Some(root_view) = window.root.clone() else {
                cx.windows.get_mut(id)?.replace(window);
                return None;
            };

            cx.window_update_stack.push(window.handle.id);
            let result = update(root_view, &mut window, cx);
            cx.window_update_stack.pop();

            if window.removed {
                cx.window_handles.remove(&id);
                cx.windows.remove(id);

                cx.window_closed_observers.clone().retain(&(), |callback| {
                    callback(cx);
                    true
                });
            } else {
                cx.windows.get_mut(id)?.replace(window);
            }

            Some(result)
        })
        .context("window not found or window has no root view")
    }

    /// Creates an `AsyncApp`, which can be cloned and has a static lifetime
    /// so it can be held across `await` points.
    pub fn to_async(&self) -> AsyncApp {
        AsyncApp {
            app: self.this.clone(),
            background_executor: self.background_executor.clone(),
            foreground_executor: self.foreground_executor.clone(),
        }
    }

    /// Obtains a reference to the executor, which can be used to spawn futures.
    pub fn background_executor(&self) -> &BackgroundExecutor {
        &self.background_executor
    }

    /// Obtains a reference to the executor, which can be used to spawn futures.
    pub fn foreground_executor(&self) -> &ForegroundExecutor {
        if self.quitting {
            panic!("Can't spawn on main thread after on_app_quit")
        };
        &self.foreground_executor
    }

    /// Spawns the future returned by the given function on the main thread. The closure will be invoked
    /// with [AsyncApp], which allows the application state to be accessed across await points.
    #[track_caller]
    pub fn spawn<AsyncFn, R>(&self, f: AsyncFn) -> Task<R>
    where
        AsyncFn: AsyncFnOnce(&mut AsyncApp) -> R + 'static,
        R: 'static,
    {
        if self.quitting {
            debug_panic!("Can't spawn on main thread after on_app_quit")
        };

        let mut cx = self.to_async();

        self.foreground_executor
            .spawn(async move { f(&mut cx).await })
    }

    /// Schedules the given function to be run at the end of the current effect cycle, allowing entities
    /// that are currently on the stack to be returned to the app.
    pub fn defer(&mut self, f: impl FnOnce(&mut App) + 'static) {
        self.push_effect(Effect::Defer {
            callback: Box::new(f),
        });
    }

    /// Accessor for the application's asset source, which is provided when constructing the `App`.
    pub fn asset_source(&self) -> &Arc<dyn AssetSource> {
        &self.asset_source
    }

    /// Accessor for the text system.
    pub fn text_system(&self) -> &Arc<TextSystem> {
        &self.text_system
    }

    /// Check whether a global of the given type has been assigned.
    pub fn has_global<G: Global>(&self) -> bool {
        self.globals_by_type.contains_key(&TypeId::of::<G>())
    }

    /// Access the global of the given type. Panics if a global for that type has not been assigned.
    #[track_caller]
    pub fn global<G: Global>(&self) -> &G {
        self.globals_by_type
            .get(&TypeId::of::<G>())
            .and_then(|any_state| any_state.downcast_ref::<G>())
            .unwrap_or_else(|| panic!("no state of type {} exists", type_name::<G>()))
    }

    /// Access the global of the given type if a value has been assigned.
    pub fn try_global<G: Global>(&self) -> Option<&G> {
        self.globals_by_type
            .get(&TypeId::of::<G>())
            .and_then(|any_state| any_state.downcast_ref::<G>())
    }

    /// Access the global of the given type mutably. Panics if a global for that type has not been assigned.
    #[track_caller]
    pub fn global_mut<G: Global>(&mut self) -> &mut G {
        let global_type = TypeId::of::<G>();
        self.push_effect(Effect::NotifyGlobalObservers { global_type });
        self.globals_by_type
            .get_mut(&global_type)
            .and_then(|any_state| any_state.downcast_mut::<G>())
            .unwrap_or_else(|| panic!("no state of type {} exists", type_name::<G>()))
    }

    /// Access the global of the given type mutably. A default value is assigned if a global of this type has not
    /// yet been assigned.
    pub fn default_global<G: Global + Default>(&mut self) -> &mut G {
        let global_type = TypeId::of::<G>();
        self.push_effect(Effect::NotifyGlobalObservers { global_type });
        self.bump_global_generation();
        self.globals_by_type
            .entry(global_type)
            .or_insert_with(|| Box::<G>::default())
            .downcast_mut::<G>()
            .unwrap_or_else(|| panic!("no state of type {} exists", type_name::<G>()))
    }

    /// Sets the value of the global of the given type.
    pub fn set_global<G: Global>(&mut self, global: G) {
        let global_type = TypeId::of::<G>();
        self.push_effect(Effect::NotifyGlobalObservers { global_type });
        self.bump_global_generation();
        self.globals_by_type.insert(global_type, Box::new(global));
    }

    /// Clear all stored globals. Does not notify global observers.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_globals(&mut self) {
        self.globals_by_type.drain();
        self.bump_global_generation();
    }

    /// Remove the global of the given type from the app context. Does not notify global observers.
    pub fn remove_global<G: Global>(&mut self) -> G {
        let global_type = TypeId::of::<G>();
        self.push_effect(Effect::NotifyGlobalObservers { global_type });
        self.bump_global_generation();
        let global = self
            .globals_by_type
            .remove(&global_type)
            .unwrap_or_else(|| panic!("no global added for {}", std::any::type_name::<G>()));
        match global.downcast() {
            Ok(global) => *global,
            Err(_) => panic!("stored global type did not match {}", type_name::<G>()),
        }
    }

    /// Register a callback to be invoked when a global of the given type is updated.
    pub fn observe_global<G: Global>(
        &mut self,
        mut f: impl FnMut(&mut Self) + 'static,
    ) -> Subscription {
        let (subscription, activate) = self.global_observers.insert(
            TypeId::of::<G>(),
            Box::new(move |cx| {
                f(cx);
                true
            }),
        );
        self.defer(move |_| activate());
        subscription
    }

    /// Move the global of the given type to the stack.
    #[track_caller]
    pub(crate) fn lease_global<G: Global>(&mut self) -> GlobalLease<G> {
        GlobalLease::new(
            self.globals_by_type
                .remove(&TypeId::of::<G>())
                .unwrap_or_else(|| panic!("no global registered of type {}", type_name::<G>())),
        )
    }

    /// Restore the global of the given type after it is moved to the stack.
    pub(crate) fn end_global_lease<G: Global>(&mut self, lease: GlobalLease<G>) {
        let global_type = TypeId::of::<G>();

        self.push_effect(Effect::NotifyGlobalObservers { global_type });
        self.bump_global_generation();
        self.globals_by_type.insert(global_type, lease.global);
    }

    pub(crate) fn new_entity_observer(
        &self,
        key: TypeId,
        value: NewEntityListener,
    ) -> Subscription {
        let (subscription, activate) = self.new_entity_observers.insert(key, value);
        activate();
        subscription
    }

    /// Arrange for the given function to be invoked whenever a view of the specified type is created.
    /// The function will be passed a mutable reference to the view along with an appropriate context.
    pub fn observe_new<T: 'static>(
        &self,
        on_new: impl 'static + Fn(&mut T, Option<&mut Window>, &mut Context<T>),
    ) -> Subscription {
        self.new_entity_observer(
            TypeId::of::<T>(),
            Box::new(
                move |any_entity: AnyEntity, window: &mut Option<&mut Window>, cx: &mut App| {
                    let Ok(entity) = any_entity.downcast::<T>() else {
                        return;
                    };
                    entity.update(cx, |entity_state, cx| {
                        on_new(entity_state, window.as_deref_mut(), cx)
                    })
                },
            ),
        )
    }

    /// Observe the release of a entity. The callback is invoked after the entity
    /// has no more strong references but before it has been dropped.
    pub fn observe_release<T>(
        &self,
        handle: &Entity<T>,
        on_release: impl FnOnce(&mut T, &mut App) + 'static,
    ) -> Subscription
    where
        T: 'static,
    {
        let (subscription, activate) = self.release_listeners.insert(
            handle.entity_id(),
            Box::new(move |entity, cx| {
                let Some(entity) = entity.downcast_mut() else {
                    return;
                };
                on_release(entity, cx)
            }),
        );
        activate();
        subscription
    }

    /// Observe the release of a entity. The callback is invoked after the entity
    /// has no more strong references but before it has been dropped.
    pub fn observe_release_in<T>(
        &self,
        handle: &Entity<T>,
        window: &Window,
        on_release: impl FnOnce(&mut T, &mut Window, &mut App) + 'static,
    ) -> Subscription
    where
        T: 'static,
    {
        let window_handle = window.handle;
        self.observe_release(handle, move |entity, cx| {
            let _ = window_handle.update(cx, |_, window, cx| on_release(entity, window, cx));
        })
    }

    /// Register a callback to be invoked when a keystroke is received by the application
    /// in any window. Note that this fires after all other action and event mechanisms have resolved
    /// and that this API will not be invoked if the event's propagation is stopped.
    pub fn observe_keystrokes(
        &mut self,
        mut f: impl FnMut(&KeystrokeEvent, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        fn inner(
            keystroke_observers: &SubscriberSet<(), KeystrokeObserver>,
            handler: KeystrokeObserver,
        ) -> Subscription {
            let (subscription, activate) = keystroke_observers.insert((), handler);
            activate();
            subscription
        }

        inner(
            &self.keystroke_observers,
            Box::new(move |event, window, cx| {
                f(event, window, cx);
                true
            }),
        )
    }

    /// Register a callback to be invoked when a keystroke is received by the application
    /// in any window. Note that this fires _before_ all other action and event mechanisms have resolved
    /// unlike [`App::observe_keystrokes`] which fires after. This means that `cx.stop_propagation` calls
    /// within interceptors will prevent action dispatch
    pub fn intercept_keystrokes(
        &mut self,
        mut f: impl FnMut(&KeystrokeEvent, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        fn inner(
            keystroke_interceptors: &SubscriberSet<(), KeystrokeObserver>,
            handler: KeystrokeObserver,
        ) -> Subscription {
            let (subscription, activate) = keystroke_interceptors.insert((), handler);
            activate();
            subscription
        }

        inner(
            &self.keystroke_interceptors,
            Box::new(move |event, window, cx| {
                f(event, window, cx);
                true
            }),
        )
    }

    /// Register key bindings.
    pub fn bind_keys(&mut self, bindings: impl IntoIterator<Item = KeyBinding>) {
        self.keymap.borrow_mut().add_bindings(bindings);
        self.pending_effects.push_back(Effect::RefreshWindows);
    }

    /// Clear all key bindings in the app.
    pub fn clear_key_bindings(&mut self) {
        self.keymap.borrow_mut().clear();
        self.pending_effects.push_back(Effect::RefreshWindows);
    }

    /// Get all key bindings in the app.
    pub fn key_bindings(&self) -> Rc<RefCell<Keymap>> {
        self.keymap.clone()
    }

    /// Register a global handler for actions invoked via the keyboard. These handlers are run at
    /// the end of the bubble phase for actions, and so will only be invoked if there are no other
    /// handlers or if they called `cx.propagate()`.
    pub fn on_action<A: Action>(&mut self, listener: impl Fn(&A, &mut Self) + 'static) {
        self.global_action_listeners
            .entry(TypeId::of::<A>())
            .or_default()
            .push(Rc::new(move |action, phase, cx| {
                if phase == DispatchPhase::Bubble {
                    let Some(action) = action.downcast_ref() else {
                        return;
                    };
                    listener(action, cx)
                }
            }));
    }

    /// Event handlers propagate events by default. Call this method to stop dispatching to
    /// event handlers with a lower z-index (mouse) or higher in the tree (keyboard). This is
    /// the opposite of [`Self::propagate`]. It's also possible to cancel a call to [`Self::propagate`] by
    /// calling this method before effects are flushed.
    pub fn stop_propagation(&mut self) {
        self.propagate_event = false;
    }

    /// Action handlers stop propagation by default during the bubble phase of action dispatch
    /// dispatching to action handlers higher in the element tree. This is the opposite of
    /// [`Self::stop_propagation`]. It's also possible to cancel a call to [`Self::stop_propagation`] by calling
    /// this method before effects are flushed.
    pub fn propagate(&mut self) {
        self.propagate_event = true;
    }

    /// Build an action from some arbitrary data, typically a keymap entry.
    pub fn build_action(
        &self,
        name: &str,
        data: Option<serde_json::Value>,
    ) -> std::result::Result<Box<dyn Action>, ActionBuildError> {
        self.actions.build_action(name, data)
    }

    /// Get all action names that have been registered. Note that registration only allows for
    /// actions to be built dynamically, and is unrelated to binding actions in the element tree.
    pub fn all_action_names(&self) -> &[&'static str] {
        self.actions.all_action_names()
    }

    /// Returns key bindings that invoke the given action on the currently focused element, without
    /// checking context. Bindings are returned in the order they were added. For display, the last
    /// binding should take precedence.
    pub fn all_bindings_for_input(&self, input: &[Keystroke]) -> Vec<KeyBinding> {
        RefCell::borrow(&self.keymap).all_bindings_for_input(input)
    }

    /// Get all non-internal actions that have been registered, along with their schemas.
    pub fn action_schemas(
        &self,
        generator: &mut schemars::SchemaGenerator,
    ) -> Vec<(&'static str, Option<schemars::Schema>)> {
        self.actions.action_schemas(generator)
    }

    /// Get a map from a deprecated action name to the canonical name.
    pub fn deprecated_actions_to_preferred_actions(&self) -> &HashMap<&'static str, &'static str> {
        self.actions.deprecated_aliases()
    }

    /// Get a map from an action name to the deprecation messages.
    pub fn action_deprecation_messages(&self) -> &HashMap<&'static str, &'static str> {
        self.actions.deprecation_messages()
    }

    /// Get a map from an action name to the documentation.
    pub fn action_documentation(&self) -> &HashMap<&'static str, &'static str> {
        self.actions.documentation()
    }

    /// Register a callback to be invoked when the application is about to quit.
    /// It is not possible to cancel the quit event at this point.
    pub fn on_app_quit<Fut>(
        &self,
        mut on_quit: impl FnMut(&mut App) -> Fut + 'static,
    ) -> Subscription
    where
        Fut: 'static + Future<Output = ()>,
    {
        let (subscription, activate) = self.quit_observers.insert(
            (),
            Box::new(move |cx| {
                let future = on_quit(cx);
                future.boxed_local()
            }),
        );
        activate();
        subscription
    }

    /// Register a callback to be invoked when the application is about to restart.
    ///
    /// These callbacks are called before any `on_app_quit` callbacks.
    pub fn on_app_restart(&self, mut on_restart: impl 'static + FnMut(&mut App)) -> Subscription {
        let (subscription, activate) = self.restart_observers.insert(
            (),
            Box::new(move |cx| {
                on_restart(cx);
                true
            }),
        );
        activate();
        subscription
    }

    /// Register a callback to be invoked when a window is closed
    /// The window is no longer accessible at the point this callback is invoked.
    pub fn on_window_closed(&self, mut on_closed: impl FnMut(&mut App) + 'static) -> Subscription {
        let (subscription, activate) = self.window_closed_observers.insert((), Box::new(on_closed));
        activate();
        subscription
    }

    pub(crate) fn clear_pending_keystrokes(&mut self) {
        for window in self.windows() {
            window
                .update(self, |_, window, _| {
                    window.clear_pending_keystrokes();
                })
                .ok();
        }
    }

    /// Returns whether the active window's focused element can undo its next shared-history change.
    pub fn has_undo(&mut self) -> bool {
        let mut has_undo = false;
        if let Some(window) = self.active_window()
            && let Ok(window_has_undo) = window.update(self, |_, window, cx| window.has_undo(cx))
        {
            has_undo = window_has_undo;
        }

        has_undo
    }

    /// Returns whether the active window's focused element can redo its next shared-history change.
    pub fn has_redo(&mut self) -> bool {
        let mut has_redo = false;
        if let Some(window) = self.active_window()
            && let Ok(window_has_redo) = window.update(self, |_, window, cx| window.has_redo(cx))
        {
            has_redo = window_has_redo;
        }

        has_redo
    }

    /// Returns the label for the active window's next focused undo action, if any.
    pub fn undo_label(&mut self) -> Option<SharedString> {
        let mut undo_label = None;
        if let Some(window) = self.active_window()
            && let Ok(window_undo_label) =
                window.update(self, |_, window, cx| window.undo_label(cx))
        {
            undo_label = window_undo_label;
        }

        undo_label
    }

    /// Returns the label for the active window's next focused redo action, if any.
    pub fn redo_label(&mut self) -> Option<SharedString> {
        let mut redo_label = None;
        if let Some(window) = self.active_window()
            && let Ok(window_redo_label) =
                window.update(self, |_, window, cx| window.redo_label(cx))
        {
            redo_label = window_redo_label;
        }

        redo_label
    }

    /// Checks if the given action is bound in the current context, as defined by the app's current focus,
    /// the bindings in the element tree, and any global action listeners.
    pub fn is_action_available(&mut self, action: &dyn Action) -> bool {
        let action_type = action.as_any().type_id();
        let action_available = if action_type == TypeId::of::<Undo>() {
            self.has_undo()
        } else if action_type == TypeId::of::<Redo>() {
            self.has_redo()
        } else {
            let mut action_available = false;
            if let Some(window) = self.active_window()
                && let Ok(window_action_available) =
                    window.update(self, |_, window, cx| window.is_action_available(action, cx))
            {
                action_available = window_action_available;
            }

            action_available
        };

        action_available || self.global_action_listeners.contains_key(&action_type)
    }

    /// Sets the menu bar for this application. This will replace any existing menu bar.
    pub fn set_menus(&self, menus: impl Into<Vec<Menu>>) {
        self.platform.set_menus(menus.into(), &self.keymap.borrow());
    }

    /// Validate and set the menu bar for this application.
    pub fn set_menus_checked(&self, menus: MenuBarBuilder) -> Result<()> {
        self.platform
            .set_menus(menus.build_checked()?, &self.keymap.borrow());
        Ok(())
    }

    /// Gets the menu bar for this application.
    pub fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.platform.get_menus()
    }

    /// Sets the right click menu for the app icon in the dock
    pub fn set_dock_menu(&self, menus: Vec<MenuItem>) {
        self.platform.set_dock_menu(menus, &self.keymap.borrow())
    }

    /// Validates and sets the right-click menu for the app icon in the dock/taskbar.
    pub fn set_dock_menu_checked(&self, menu: DockMenuBuilder) -> Result<()> {
        let menu = menu.build_checked()?;
        self.platform.set_dock_menu(menu, &self.keymap.borrow());
        Ok(())
    }

    /// Performs the action associated with the given dock menu item.
    /// This is currently only used on Windows.
    pub fn perform_dock_menu_action(&self, action: usize) {
        self.platform.perform_dock_menu_action(action);
    }

    /// Adds given path to the bottom of the list of recent paths for the application.
    /// The list is usually shown on the application icon's context menu in the dock,
    /// and allows to open the recent files via that context menu.
    /// If the path is already in the list, it will be moved to the bottom of the list.
    pub fn add_recent_document(&self, path: &Path) {
        self.platform.add_recent_document(path);
    }

    /// Adds multiple paths to the OS recent-documents list using a builder-friendly API.
    pub fn add_recent_documents(
        &self,
        documents: impl Into<RecentDocumentsBuilder>,
    ) -> Result<Vec<PathBuf>> {
        let documents = documents.into().build()?;
        for path in &documents {
            self.add_recent_document(path);
        }
        Ok(documents)
    }

    /// Updates the jump list with the updated list of recent paths for the application.
    /// This is currently only used on Windows.
    /// Note that this also sets the dock menu on Windows.
    pub fn update_jump_list(
        &self,
        menus: Vec<MenuItem>,
        entries: Vec<SmallVec<[PathBuf; 2]>>,
    ) -> Vec<SmallVec<[PathBuf; 2]>> {
        self.platform.update_jump_list(menus, entries)
    }

    /// Validates and updates the jump list with recent paths and task actions.
    /// This is currently only used on Windows.
    pub fn update_jump_list_checked(
        &self,
        jump_list: JumpListBuilder,
    ) -> Result<Vec<SmallVec<[PathBuf; 2]>>> {
        let (menus, entries) = jump_list.build_checked()?;
        Ok(self.update_jump_list(menus, entries))
    }

    /// Dispatch an action to the currently active window or global action handler
    /// See [`crate::Action`] for more information on how actions work
    pub fn dispatch_action(&mut self, action: &dyn Action) {
        if let Some(active_window) = self.active_window() {
            active_window
                .update(self, |_, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx)
                })
                .log_err();
        } else {
            self.dispatch_global_action(action);
        }
    }

    fn dispatch_global_action(&mut self, action: &dyn Action) {
        self.propagate_event = true;

        if let Some(mut global_listeners) = self
            .global_action_listeners
            .remove(&action.as_any().type_id())
        {
            for listener in &global_listeners {
                listener(action.as_any(), DispatchPhase::Capture, self);
                if !self.propagate_event {
                    break;
                }
            }

            global_listeners.extend(
                self.global_action_listeners
                    .remove(&action.as_any().type_id())
                    .unwrap_or_default(),
            );

            self.global_action_listeners
                .insert(action.as_any().type_id(), global_listeners);
        }

        if self.propagate_event
            && let Some(mut global_listeners) = self
                .global_action_listeners
                .remove(&action.as_any().type_id())
        {
            for listener in global_listeners.iter().rev() {
                listener(action.as_any(), DispatchPhase::Bubble, self);
                if !self.propagate_event {
                    break;
                }
            }

            global_listeners.extend(
                self.global_action_listeners
                    .remove(&action.as_any().type_id())
                    .unwrap_or_default(),
            );

            self.global_action_listeners
                .insert(action.as_any().type_id(), global_listeners);
        }
    }

    /// Is there currently something being dragged?
    pub fn has_active_drag(&self) -> bool {
        self.active_drag.is_some()
    }

    /// Gets the cursor style of the currently active drag operation.
    pub fn active_drag_cursor_style(&self) -> Option<CursorStyle> {
        self.active_drag.as_ref().and_then(|drag| drag.cursor_style)
    }

    /// Stops active drag and clears any related effects.
    pub fn stop_active_drag(&mut self, window: &mut Window) -> bool {
        if self.active_drag.is_some() {
            self.active_drag = None;
            window.refresh();
            true
        } else {
            false
        }
    }

    /// Sets the cursor style for the currently active drag operation.
    pub fn set_active_drag_cursor_style(
        &mut self,
        cursor_style: CursorStyle,
        window: &mut Window,
    ) -> bool {
        if let Some(ref mut drag) = self.active_drag {
            drag.cursor_style = Some(cursor_style);
            window.refresh();
            true
        } else {
            false
        }
    }

    /// Set the prompt renderer for GPUI. This will replace the default or platform specific
    /// prompts with this custom implementation.
    pub fn set_prompt_builder(
        &mut self,
        renderer: impl Fn(
            PromptLevel,
            &str,
            Option<&str>,
            &[PromptButton],
            PromptHandle,
            &mut Window,
            &mut App,
        ) -> RenderablePromptHandle
        + 'static,
    ) {
        self.prompt_builder = Some(PromptBuilder::Custom(Box::new(renderer)));
    }

    /// Reset the prompt builder to the default implementation.
    pub fn reset_prompt_builder(&mut self) {
        self.prompt_builder = Some(PromptBuilder::Default);
    }

    /// Set the permission broker for capability checks.
    pub fn set_permission_broker(&mut self, broker: PermissionBroker) {
        self.permission_broker = broker;
    }

    /// Get a reference to the permission broker.
    pub fn permission_broker(&self) -> &PermissionBroker {
        &self.permission_broker
    }

    /// Get the current process identifier used for capability checks.
    pub fn current_process_id(&self) -> ProcessId {
        self.current_process_id
    }

    /// Set the current process identifier used for capability checks.
    pub fn set_current_process_id(&mut self, process_id: ProcessId) {
        self.current_process_id = process_id;
    }

    /// Remove an asset from GPUI's cache
    pub fn remove_asset<A: Asset>(&mut self, source: &A::Source) {
        let asset_id = (TypeId::of::<A>(), hash(source));
        self.loading_assets.remove(&asset_id);
    }

    /// Asynchronously load an asset, if the asset hasn't finished loading this will return None.
    ///
    /// Note that the multiple calls to this method will only result in one `Asset::load` call at a
    /// time, and the results of this call will be cached
    pub fn fetch_asset<A: Asset>(&mut self, source: &A::Source) -> (Shared<Task<A::Output>>, bool) {
        let asset_id = (TypeId::of::<A>(), hash(source));
        let mut is_first = false;
        let task = self
            .loading_assets
            .remove(&asset_id)
            .map(
                |boxed_task| match boxed_task.downcast::<Shared<Task<A::Output>>>() {
                    Ok(task) => *task,
                    Err(_) => panic!("stored asset task type did not match {}", type_name::<A>()),
                },
            )
            .unwrap_or_else(|| {
                is_first = true;
                let future = A::load(source.clone(), self);

                self.background_executor().spawn(future).shared()
            });

        self.loading_assets.insert(asset_id, Box::new(task.clone()));

        (task, is_first)
    }

    /// Obtain a new [`FocusHandle`], which allows you to track and manipulate the keyboard focus
    /// for elements rendered within this window.
    #[track_caller]
    pub fn focus_handle(&self) -> FocusHandle {
        FocusHandle::new(&self.focus_handles)
    }

    /// Tell GPUI that an entity has changed and observers of it should be notified.
    pub fn notify(&mut self, entity_id: EntityId) {
        self.entities.bump_generation(entity_id);

        let window_invalidators = mem::take(
            self.window_invalidators_by_entity
                .entry(entity_id)
                .or_default(),
        );

        if window_invalidators.is_empty() {
            if self.pending_notifications.insert(entity_id) {
                self.pending_effects
                    .push_back(Effect::Notify { emitter: entity_id });
            }
        } else {
            for invalidator in window_invalidators.values() {
                invalidator.invalidate_view(entity_id, self);
            }
        }

        self.window_invalidators_by_entity
            .insert(entity_id, window_invalidators);
    }

    pub(crate) fn entity_generation(&self, entity_id: EntityId) -> Option<u64> {
        self.entities.generation(entity_id)
    }

    pub(crate) fn global_generation(&self) -> u64 {
        self.global_generation
    }

    fn bump_global_generation(&mut self) {
        self.global_generation = self.global_generation.wrapping_add(1);
    }

    /// Returns the name for this [`App`].
    #[cfg(any(test, feature = "test-support", debug_assertions))]
    pub fn get_name(&self) -> Option<&'static str> {
        self.name
    }

    /// Returns `true` if the platform file picker supports selecting a mix of files and directories.
    pub fn can_select_mixed_files_and_dirs(&self) -> bool {
        self.platform.can_select_mixed_files_and_dirs()
    }

    /// Removes an image from the sprite atlas on all windows.
    ///
    /// If the current window is being updated, it will be removed from `App.windows`, you can use `current_window` to specify the current window.
    /// This is a no-op if the image is not in the sprite atlas.
    pub fn drop_image(&mut self, image: Arc<RenderImage>, current_window: Option<&mut Window>) {
        // remove the texture from all other windows
        for window in self.windows.values_mut().flatten() {
            _ = window.drop_image(image.clone());
        }

        // remove the texture from the current window
        if let Some(window) = current_window {
            _ = window.drop_image(image);
        }
    }

    /// Sets the renderer for the inspector.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn set_inspector_renderer(&mut self, f: crate::InspectorRenderer) {
        self.inspector_renderer = Some(f);
    }

    /// Registers a renderer specific to an inspector state.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn register_inspector_element<T: 'static, R: crate::IntoElement>(
        &mut self,
        f: impl 'static + Fn(crate::InspectorElementId, &T, &mut Window, &mut App) -> R,
    ) {
        self.inspector_element_registry.register(f);
    }

    /// Watches a JSON or TOML theme file and applies each successful reload.
    pub fn observe_theme_file(
        &mut self,
        path: impl AsRef<Path>,
        mut on_change: impl FnMut(Theme, &mut App) + 'static,
    ) -> Result<()> {
        Theme::init(self);

        let theme_path = normalize_theme_path(path.as_ref())?;
        let watch_root = theme_path
            .parent()
            .with_context(|| {
                format!(
                    "theme file {} has no parent directory",
                    theme_path.display()
                )
            })?
            .to_path_buf();

        apply_theme_file_change(self, &theme_path, &mut on_change)?;

        let watched_path_for_reload = theme_path.clone();
        let app = self.this.clone();
        let mut watcher = FileWatcher::new(self, move |event| {
            let Some(app) = app.upgrade() else {
                return;
            };

            let cx = &mut app.borrow_mut();

            match handle_theme_file_event(cx, &event, &watched_path_for_reload, &mut on_change) {
                Ok(_) => {}
                Err(error) => {
                    log::error!(
                        "failed to reload theme file {}: {error:#}",
                        watched_path_for_reload.display()
                    );
                }
            }
        })?;
        watcher.watch(&watch_root, false)?;

        retain_file_watcher(self, watcher);

        Ok(())
    }

    /// Registers a subscriber that is notified whenever a watched theme file
    /// reloads, after the core [`Theme`] global has been updated.
    ///
    /// This is the pluggable sink that lets higher layers bridge the
    /// file-facing [`Theme`] onto their own runtime representation. The
    /// callback receives the freshly parsed [`Theme`] and a mutable [`App`],
    /// and runs for every theme file watched through
    /// [`App::observe_theme_file`]. The returned [`Subscription`] removes the
    /// subscriber when dropped; call [`Subscription::detach`] to keep it active
    /// for the lifetime of the app.
    pub fn observe_theme_files(
        &mut self,
        callback: impl FnMut(&Theme, &mut App) + 'static,
    ) -> Subscription {
        Theme::init(self);
        register_theme_file_subscriber(self, Box::new(callback))
    }

    /// Initializes gpui's default theme-derived colors for the application.
    ///
    /// These colors can be accessed through `cx.default_colors()`.
    pub fn init_colors(&mut self) {
        Theme::init(self);
    }
}

fn apply_theme_file_change(
    cx: &mut App,
    theme_path: &Path,
    on_change: &mut dyn FnMut(Theme, &mut App),
) -> Result<()> {
    let theme = Theme::from_path(theme_path)?;
    on_change(theme.clone(), cx);
    crate::theme::notify_theme_file_subscribers(cx, &theme);
    cx.flush_effects();
    Ok(())
}

pub(crate) fn handle_theme_file_event(
    cx: &mut App,
    event: &crate::FileWatchEvent,
    watched_path: &Path,
    on_change: &mut dyn FnMut(Theme, &mut App),
) -> Result<bool> {
    if !theme_file_event_matches_target(event, watched_path) {
        return Ok(false);
    }

    apply_theme_file_change(cx, watched_path, on_change)?;
    Ok(true)
}

impl AppContext for App {
    type Result<T> = T;

    /// Builds an entity that is owned by the application.
    ///
    /// The given function will be invoked with a [`Context`] and must return an object representing the entity. An
    /// [`Entity`] handle will be returned, which can be used to access the entity in a context.
    fn new<T: 'static>(&mut self, build_entity: impl FnOnce(&mut Context<T>) -> T) -> Entity<T> {
        self.update(|cx| {
            let slot = cx.entities.reserve();
            let handle = slot.clone();
            let entity = build_entity(&mut Context::new_context(cx, slot.downgrade()));

            cx.push_effect(Effect::EntityCreated {
                entity: handle.clone().into_any(),
                tid: TypeId::of::<T>(),
                window: cx.window_update_stack.last().cloned(),
            });

            cx.entities.insert(slot, entity);
            handle
        })
    }

    fn reserve_entity<T: 'static>(&mut self) -> Self::Result<Reservation<T>> {
        Reservation(self.entities.reserve())
    }

    fn insert_entity<T: 'static>(
        &mut self,
        reservation: Reservation<T>,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>> {
        self.update(|cx| {
            let slot = reservation.0;
            let entity = build_entity(&mut Context::new_context(cx, slot.downgrade()));
            cx.entities.insert(slot, entity)
        })
    }

    /// Updates the entity referenced by the given handle. The function is passed a mutable reference to the
    /// entity along with a `Context` for the entity.
    fn update_entity<T: 'static, R>(
        &mut self,
        handle: &Entity<T>,
        update: impl FnOnce(&mut T, &mut Context<T>) -> R,
    ) -> R {
        self.update(|cx| {
            let mut entity = cx.entities.lease(handle);
            let result = update(
                &mut entity,
                &mut Context::new_context(cx, handle.downgrade()),
            );
            cx.entities.end_lease(entity);
            result
        })
    }

    fn as_mut<'a, T>(&'a mut self, handle: &Entity<T>) -> GpuiBorrow<'a, T>
    where
        T: 'static,
    {
        GpuiBorrow::new(handle.clone(), self)
    }

    fn read_entity<T, R>(
        &self,
        handle: &Entity<T>,
        read: impl FnOnce(&T, &App) -> R,
    ) -> Self::Result<R>
    where
        T: 'static,
    {
        let entity = self.entities.read(handle);
        read(entity, self)
    }

    fn update_window<T, F>(&mut self, handle: AnyWindowHandle, update: F) -> Result<T>
    where
        F: FnOnce(AnyView, &mut Window, &mut App) -> T,
    {
        self.update_window_id(handle.id, update)
    }

    fn read_window<T, R>(
        &self,
        window: &WindowHandle<T>,
        read: impl FnOnce(Entity<T>, &App) -> R,
    ) -> Result<R>
    where
        T: 'static,
    {
        let window = self
            .windows
            .get(window.id)
            .context("window not found")?
            .as_ref()
            .context("attempted to read a window that is already on the stack")?;

        let root_view = window.root.clone().context("window has no root view")?;
        let view = root_view
            .downcast::<T>()
            .map_err(|_| anyhow!("root view's type has changed"))?;

        Ok(read(view, self))
    }

    fn background_spawn<R>(&self, future: impl Future<Output = R> + Send + 'static) -> Task<R>
    where
        R: Send + 'static,
    {
        self.background_executor.spawn(future)
    }

    fn read_global<G, R>(&self, callback: impl FnOnce(&G, &App) -> R) -> Self::Result<R>
    where
        G: Global,
    {
        let mut g = self.global::<G>();
        callback(g, self)
    }
}

/// These effects are processed at the end of each application update cycle.
pub(crate) enum Effect {
    Notify {
        emitter: EntityId,
    },
    Emit {
        emitter: EntityId,
        event_type: TypeId,
        event: Box<dyn Any>,
    },
    RefreshWindows,
    NotifyGlobalObservers {
        global_type: TypeId,
    },
    Defer {
        callback: Box<dyn FnOnce(&mut App) + 'static>,
    },
    EntityCreated {
        entity: AnyEntity,
        tid: TypeId,
        window: Option<WindowId>,
    },
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Effect::Notify { emitter } => write!(f, "Notify({})", emitter),
            Effect::Emit { emitter, .. } => write!(f, "Emit({:?})", emitter),
            Effect::RefreshWindows => write!(f, "RefreshWindows"),
            Effect::NotifyGlobalObservers { global_type } => {
                write!(f, "NotifyGlobalObservers({:?})", global_type)
            }
            Effect::Defer { .. } => write!(f, "Defer(..)"),
            Effect::EntityCreated { entity, .. } => write!(f, "EntityCreated({:?})", entity),
        }
    }
}

/// Wraps a global variable value during `update_global` while the value has been moved to the stack.
pub(crate) struct GlobalLease<G: Global> {
    global: Box<dyn Any>,
    global_type: PhantomData<G>,
}

impl<G: Global> GlobalLease<G> {
    fn new(global: Box<dyn Any>) -> Self {
        GlobalLease {
            global,
            global_type: PhantomData,
        }
    }
}

impl<G: Global> Deref for GlobalLease<G> {
    type Target = G;

    fn deref(&self) -> &Self::Target {
        self.global.downcast_ref().unwrap_or_else(|| {
            panic!(
                "stored global lease type did not match {}",
                type_name::<G>()
            )
        })
    }
}

impl<G: Global> DerefMut for GlobalLease<G> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.global.downcast_mut().unwrap_or_else(|| {
            panic!(
                "stored global lease type did not match {}",
                type_name::<G>()
            )
        })
    }
}

/// Contains state associated with an active drag operation, started by dragging an element
/// within the window or by dragging into the app from the underlying platform.
pub struct AnyDrag {
    /// The view used to render this drag
    pub view: AnyView,

    /// The value of the dragged item, to be dropped
    pub value: Arc<dyn Any>,

    /// This is used to render the dragged item in the same place
    /// on the original element that the drag was initiated
    pub cursor_offset: Point<Pixels>,

    /// The cursor style to use while dragging
    pub cursor_style: Option<CursorStyle>,
}

/// Contains state associated with a tooltip. You'll only need this struct if you're implementing
/// tooltip behavior on a custom element. Otherwise, use [Div::tooltip](crate::Interactivity::tooltip).
#[derive(Clone)]
pub struct AnyTooltip {
    /// The view used to display the tooltip
    pub view: AnyView,

    /// The absolute position of the mouse when the tooltip was deployed.
    pub mouse_position: Point<Pixels>,

    /// Given the bounds of the tooltip, checks whether the tooltip should still be visible and
    /// updates its state accordingly. This is needed atop the hovered element's mouse move handler
    /// to handle the case where the element is not painted (e.g. via use of `visible_on_hover`).
    pub check_visible_and_update: Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App) -> bool>,
}

/// A keystroke event, and potentially the associated action
#[derive(Debug)]
pub struct KeystrokeEvent {
    /// The keystroke that occurred
    pub keystroke: Keystroke,

    /// The action that was resolved for the keystroke, if any
    pub action: Option<Box<dyn Action>>,

    /// The context stack at the time
    pub context_stack: Vec<KeyContext>,
}

struct NullHttpClient;

impl HttpClient for NullHttpClient {
    fn send(
        &self,
        _req: http_client::Request<http_client::AsyncBody>,
    ) -> futures::future::BoxFuture<
        'static,
        anyhow::Result<http_client::Response<http_client::AsyncBody>>,
    > {
        async move {
            anyhow::bail!("No HttpClient available");
        }
        .boxed()
    }

    fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn type_name(&self) -> &'static str {
        type_name::<Self>()
    }
}

/// A mutable reference to an entity owned by GPUI
pub struct GpuiBorrow<'a, T> {
    inner: Option<Lease<T>>,
    app: &'a mut App,
}

impl<'a, T: 'static> GpuiBorrow<'a, T> {
    fn new(inner: Entity<T>, app: &'a mut App) -> Self {
        app.start_update();
        let lease = app.entities.lease(&inner);
        Self {
            inner: Some(lease),
            app,
        }
    }

    fn lease(&self) -> &Lease<T> {
        self.inner
            .as_ref()
            .unwrap_or_else(|| panic!("gpui borrow missing entity lease"))
    }

    fn lease_mut(&mut self) -> &mut Lease<T> {
        self.inner
            .as_mut()
            .unwrap_or_else(|| panic!("gpui borrow missing entity lease"))
    }
}

impl<'a, T: 'static> std::borrow::Borrow<T> for GpuiBorrow<'a, T> {
    fn borrow(&self) -> &T {
        self.lease().borrow()
    }
}

impl<'a, T: 'static> std::borrow::BorrowMut<T> for GpuiBorrow<'a, T> {
    fn borrow_mut(&mut self) -> &mut T {
        self.lease_mut().borrow_mut()
    }
}

impl<'a, T: 'static> std::ops::Deref for GpuiBorrow<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.lease()
    }
}

impl<'a, T: 'static> std::ops::DerefMut for GpuiBorrow<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.lease_mut()
    }
}

impl<'a, T> Drop for GpuiBorrow<'a, T> {
    fn drop(&mut self) {
        let lease = self
            .inner
            .take()
            .unwrap_or_else(|| panic!("gpui borrow missing entity lease during drop"));
        self.app.notify(lease.id);
        self.app.entities.end_lease(lease);
        self.app.finish_update();
    }
}

fn url_scheme(url: &str) -> &str {
    url.split("://").next().unwrap_or(url)
}

fn optional_u64_label(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn classify_open_request(raw: &str) -> OpenRequestKind {
    if let Some(path) = file_url_to_path(raw) {
        return OpenRequestKind::File { path };
    }

    let Some(scheme) = open_request_scheme(raw) else {
        return OpenRequestKind::Unknown;
    };

    if validate_url_scheme(scheme).is_err() {
        return OpenRequestKind::Unknown;
    }

    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" | "mailto" => OpenRequestKind::Url {
            scheme: scheme.to_string(),
        },
        _ => OpenRequestKind::DeepLink {
            scheme: scheme.to_string(),
        },
    }
}

fn open_request_scheme(raw: &str) -> Option<&str> {
    let colon = raw.find(':')?;
    let scheme = &raw[..colon];
    (!scheme.is_empty()).then_some(scheme)
}

fn file_url_to_path(raw: &str) -> Option<PathBuf> {
    if !raw
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        return None;
    }

    let without_fragment = raw
        .split_once('#')
        .map_or(raw, |(before_fragment, _)| before_fragment);
    let without_query = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(before_query, _)| before_query);
    let mut rest = &without_query[5..];

    if let Some(stripped) = rest.strip_prefix("//") {
        rest = stripped;
        if let Some(local) = rest.strip_prefix("localhost/") {
            return Some(PathBuf::from(percent_decode_open_component(&format!(
                "/{local}"
            ))));
        } else if rest.starts_with('/') {
            return Some(PathBuf::from(percent_decode_open_component(rest)));
        } else if !rest.starts_with('/') {
            return Some(PathBuf::from(format!(
                "//{}",
                percent_decode_open_component(rest)
            )));
        }
    }

    Some(PathBuf::from(percent_decode_open_component(rest)))
}

fn percent_decode_open_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            output.push((high << 4) | low);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_url_scheme(scheme: &str) -> Result<()> {
    anyhow::ensure!(!scheme.is_empty(), "URL scheme cannot be empty");
    anyhow::ensure!(
        scheme
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic()),
        "URL scheme must start with an ASCII letter: {scheme:?}"
    );
    anyhow::ensure!(
        scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        }),
        "URL scheme may only contain ASCII letters, digits, '+', '-', or '.': {scheme:?}"
    );
    Ok(())
}

fn validate_custom_protocol_scheme(scheme: &str) -> Result<()> {
    validate_url_scheme(scheme)?;
    anyhow::ensure!(
        !matches!(
            scheme,
            "http" | "https" | "file" | "data" | "javascript" | "mailto" | "about" | "blob"
        ),
        "custom protocol scheme cannot shadow a standard URL scheme: {scheme}"
    );
    Ok(())
}

fn validate_custom_protocol_root(path: &Path, require_existing_dir: bool) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "custom protocol file root cannot be empty"
    );
    if let Some(path) = path.to_str() {
        anyhow::ensure!(
            !path.contains('\0'),
            "custom protocol file root cannot contain NUL bytes"
        );
    }
    if require_existing_dir {
        let metadata = fs::metadata(path).with_context(|| {
            format!(
                "custom protocol file root does not exist: {}",
                path.display()
            )
        })?;
        anyhow::ensure!(
            metadata.is_dir(),
            "custom protocol file root must be a directory: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_custom_protocol_host(host: &str) -> Result<()> {
    anyhow::ensure!(
        !host.trim().is_empty(),
        "custom protocol host cannot be empty"
    );
    anyhow::ensure!(
        host == host.trim(),
        "custom protocol host cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        host.len() <= 253,
        "custom protocol host cannot be longer than 253 bytes"
    );
    anyhow::ensure!(
        host.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.')),
        "custom protocol host must contain only ASCII letters, digits, '-' or '.'"
    );
    anyhow::ensure!(
        !host.starts_with('.') && !host.ends_with('.'),
        "custom protocol host cannot start or end with '.'"
    );
    anyhow::ensure!(
        !host.split('.').any(str::is_empty),
        "custom protocol host cannot contain empty labels"
    );
    Ok(())
}

fn validate_custom_protocol_relative_file(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(!value.contains('\0'), "{label} cannot contain NUL bytes");
    let mut components = Path::new(value).components();
    let Some(Component::Normal(_)) = components.next() else {
        anyhow::bail!("{label} must be a relative file name");
    };
    anyhow::ensure!(
        components.next().is_none(),
        "{label} must not contain path separators"
    );
    Ok(())
}

fn ensure_path_stays_under_root(root: &Path, candidate: &Path) -> Result<()> {
    anyhow::ensure!(
        candidate.starts_with(root),
        "custom protocol file path escaped resolver root: {}",
        candidate.display()
    );
    if candidate.exists() {
        let canonical_root = root.canonicalize().with_context(|| {
            format!(
                "could not canonicalize custom protocol root {}",
                root.display()
            )
        })?;
        let canonical_candidate = candidate.canonicalize().with_context(|| {
            format!(
                "could not canonicalize custom protocol file {}",
                candidate.display()
            )
        })?;
        anyhow::ensure!(
            canonical_candidate.starts_with(&canonical_root),
            "custom protocol file path escaped resolver root: {}",
            candidate.display()
        );
    }
    Ok(())
}

fn custom_protocol_raw_path_has_parent_component(raw_url: &str) -> bool {
    let after_scheme = raw_url
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(raw_url);
    let path_with_query = if let Some(after_authority) = after_scheme.strip_prefix("//") {
        after_authority
            .split_once('/')
            .map(|(_, path)| path)
            .unwrap_or("")
    } else {
        after_scheme.trim_start_matches('/')
    };
    let path = path_with_query
        .split(['?', '#'])
        .next()
        .unwrap_or(path_with_query);
    let decoded = percent_decode_open_component(path);
    decoded.split('/').any(|segment| segment == "..")
}

fn mime_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn validate_mime_type(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.chars().count() <= 128,
        "{label} cannot be longer than 128 characters"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    let essence = value.split(';').next().unwrap_or(value);
    anyhow::ensure!(
        essence.contains('/'),
        "{label} must include a type and subtype"
    );
    Ok(())
}

fn validate_header_name(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.trim().is_empty(),
        "custom protocol header name cannot be empty"
    );
    anyhow::ensure!(
        value == value.trim(),
        "custom protocol header name cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.len() <= 128,
        "custom protocol header name cannot be longer than 128 bytes"
    );
    anyhow::ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-')),
        "custom protocol header name must contain only ASCII letters, digits, or '-'"
    );
    Ok(())
}

fn validate_header_value(value: &str) -> Result<()> {
    anyhow::ensure!(
        value.chars().count() <= 1024,
        "custom protocol header value cannot be longer than 1024 characters"
    );
    anyhow::ensure!(
        !value
            .chars()
            .any(|character| character.is_control() && character != '\t'),
        "custom protocol header value cannot contain control characters"
    );
    Ok(())
}

fn validate_app_path_id(app_id: &str) -> Result<()> {
    anyhow::ensure!(!app_id.trim().is_empty(), "app path id cannot be empty");
    anyhow::ensure!(
        app_id == app_id.trim(),
        "app path id cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !app_id
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace()),
        "app path id cannot contain whitespace or control characters"
    );
    anyhow::ensure!(
        !app_id
            .chars()
            .any(|ch| matches!(ch, '/' | '\\' | ':' | '\0')),
        "app path id cannot contain path separators or ':'"
    );
    Ok(())
}

fn validate_app_storage_id(value: &str, label: &str) -> Result<()> {
    validate_app_metadata_text(value, label, 64, false)?;
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')),
        "{label} must contain only ASCII letters, digits, '.', '-', or '_'"
    );
    Ok(())
}

fn validate_app_storage_relative_path(path: &Path) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "app storage relative path cannot be empty"
    );
    validate_no_nul(&path.to_string_lossy(), "app storage relative path")?;
    anyhow::ensure!(
        !path.is_absolute(),
        "app storage relative path cannot be absolute"
    );
    anyhow::ensure!(
        path.components().count() <= 8,
        "app storage relative path cannot contain more than 8 components"
    );
    for component in path.components() {
        let Component::Normal(name) = component else {
            anyhow::bail!("app storage relative path cannot contain '.', '..', prefixes, or roots");
        };
        let name = name.to_string_lossy();
        validate_app_metadata_text(&name, "app storage path component", 255, false)?;
        anyhow::ensure!(
            !name.contains('/') && !name.contains('\\') && !name.contains(':'),
            "app storage path component cannot contain path separators or ':'"
        );
    }
    Ok(())
}

fn validate_non_empty_path(path: &Path, label: &str) -> Result<()> {
    let value = path.to_string_lossy();
    anyhow::ensure!(!value.is_empty(), "{label} cannot be empty");
    validate_no_nul(&value, label)
}

fn validate_app_metadata_text(
    value: &str,
    label: &str,
    max_chars: usize,
    allow_newlines: bool,
) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.chars().count() <= max_chars,
        "{label} cannot be longer than {max_chars} characters"
    );
    anyhow::ensure!(
        !value
            .chars()
            .any(|ch| { ch.is_control() && !(allow_newlines && matches!(ch, '\n' | '\r' | '\t')) }),
        "{label} cannot contain control characters"
    );
    Ok(())
}

fn validate_app_metadata_url(value: &str, label: &str) -> Result<()> {
    validate_app_metadata_text(value, label, 2048, false)?;
    let url = Url::parse(value).with_context(|| format!("{label} must be a valid URL"))?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https"),
        "{label} must use http or https"
    );
    anyhow::ensure!(url.host().is_some(), "{label} must include a host");
    Ok(())
}

fn validate_launch_args(args: &[String]) -> Result<()> {
    for arg in args {
        validate_no_nul(arg, "launch argument")?;
    }
    Ok(())
}

fn validate_environment_key(key: &str) -> Result<()> {
    anyhow::ensure!(
        !key.trim().is_empty(),
        "launch context environment key cannot be empty"
    );
    anyhow::ensure!(
        key == key.trim(),
        "launch context environment key cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !key.contains('='),
        "launch context environment key cannot contain '='"
    );
    validate_no_nul(key, "launch context environment key")
}

fn validate_no_nul(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.contains('\0'), "{label} cannot contain NUL bytes");
    Ok(())
}

fn validate_text_checking_text(text: &str) -> Result<()> {
    anyhow::ensure!(!text.is_empty(), "text checking text cannot be empty");
    validate_no_nul(text, "text checking text")?;
    anyhow::ensure!(
        text.chars().count() <= 200_000,
        "text checking text cannot be longer than 200000 characters"
    );
    Ok(())
}

fn validate_custom_dictionary_words(words: &[String]) -> Result<()> {
    anyhow::ensure!(
        words.len() <= 1024,
        "custom dictionary cannot contain more than 1024 words"
    );
    let mut seen = std::collections::HashSet::new();
    for word in words {
        anyhow::ensure!(
            !word.trim().is_empty(),
            "custom dictionary word cannot be empty"
        );
        anyhow::ensure!(
            word == word.trim(),
            "custom dictionary word cannot have leading or trailing whitespace"
        );
        validate_no_nul(word, "custom dictionary word")?;
        anyhow::ensure!(
            word.chars().count() <= 128,
            "custom dictionary word cannot be longer than 128 characters"
        );
        anyhow::ensure!(
            !word.chars().any(char::is_control),
            "custom dictionary word cannot contain control characters"
        );
        let normalized = word.to_lowercase();
        anyhow::ensure!(
            seen.insert(normalized),
            "custom dictionary word configured more than once: {word}"
        );
    }
    Ok(())
}

fn validate_location_purpose(purpose: &str) -> Result<()> {
    anyhow::ensure!(
        !purpose.trim().is_empty(),
        "location purpose cannot be empty"
    );
    anyhow::ensure!(
        purpose == purpose.trim(),
        "location purpose cannot have leading or trailing whitespace"
    );
    validate_no_nul(purpose, "location purpose")?;
    anyhow::ensure!(
        purpose.chars().count() <= 240,
        "location purpose cannot be longer than 240 characters"
    );
    anyhow::ensure!(
        !purpose.chars().any(char::is_control),
        "location purpose cannot contain control characters"
    );
    Ok(())
}

fn validate_device_access_purpose(purpose: &str) -> Result<()> {
    anyhow::ensure!(
        !purpose.trim().is_empty(),
        "device access purpose cannot be empty"
    );
    anyhow::ensure!(
        purpose == purpose.trim(),
        "device access purpose cannot have leading or trailing whitespace"
    );
    validate_no_nul(purpose, "device access purpose")?;
    anyhow::ensure!(
        purpose.chars().count() <= 240,
        "device access purpose cannot be longer than 240 characters"
    );
    anyhow::ensure!(
        !purpose.chars().any(char::is_control),
        "device access purpose cannot contain control characters"
    );
    Ok(())
}

fn validate_device_port_hint(port_name_hint: &str) -> Result<()> {
    validate_no_nul(port_name_hint, "serial port hint")?;
    anyhow::ensure!(
        !port_name_hint.trim().is_empty(),
        "serial port hint cannot be empty"
    );
    anyhow::ensure!(
        port_name_hint == port_name_hint.trim(),
        "serial port hint cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        port_name_hint.chars().count() <= 128,
        "serial port hint cannot be longer than 128 characters"
    );
    anyhow::ensure!(
        !port_name_hint.chars().any(char::is_control),
        "serial port hint cannot contain control characters"
    );
    Ok(())
}

fn validate_export_drag_path(path: &Path, require_existing: bool) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "file export drag path cannot be empty"
    );
    validate_no_nul(&path.to_string_lossy(), "file export drag path")?;
    if require_existing {
        anyhow::ensure!(
            path.exists(),
            "file export drag path must exist: {}",
            path.display()
        );
        anyhow::ensure!(
            path.is_file(),
            "file export drag path must be a file: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_export_file_name(file_name: &str) -> Result<()> {
    validate_app_metadata_text(file_name, "file export drag file name", 255, false)?;
    anyhow::ensure!(
        !file_name.contains('/') && !file_name.contains('\\') && !file_name.contains(':'),
        "file export drag file name cannot contain path separators or ':'"
    );
    anyhow::ensure!(
        file_name != "." && file_name != "..",
        "file export drag file name must be a file name"
    );
    Ok(())
}

fn validate_export_mime_type(mime_type: &str) -> Result<()> {
    validate_app_metadata_text(mime_type, "file export drag MIME type", 128, false)?;
    let Some((major, sub)) = mime_type.split_once('/') else {
        anyhow::bail!("file export drag MIME type must use type/subtype form");
    };
    anyhow::ensure!(
        !major.is_empty() && !sub.is_empty(),
        "file export drag MIME type must include type and subtype"
    );
    anyhow::ensure!(
        major
            .chars()
            .chain(sub.chars())
            .all(|ch| ch.is_ascii_alphanumeric()
                || matches!(ch, '!' | '#' | '$' | '&' | '-' | '^' | '_' | '.' | '+')),
        "file export drag MIME type contains unsupported characters"
    );
    Ok(())
}

fn validate_capture_dimension(value: u32, label: &str) -> Result<()> {
    anyhow::ensure!(value > 0, "{label} must be greater than zero");
    anyhow::ensure!(value <= 16_384, "{label} cannot exceed 16384");
    Ok(())
}

fn validate_bluetooth_service_uuid(service_uuid: &str) -> Result<()> {
    validate_no_nul(service_uuid, "Bluetooth service UUID")?;
    let uuid = service_uuid.trim();
    anyhow::ensure!(
        uuid == service_uuid,
        "Bluetooth service UUID cannot have leading or trailing whitespace"
    );
    let hex = uuid.replace('-', "");
    anyhow::ensure!(
        matches!(hex.len(), 4 | 8 | 32),
        "Bluetooth service UUID must be 16-bit, 32-bit, or 128-bit"
    );
    anyhow::ensure!(
        hex.chars().all(|ch| ch.is_ascii_hexdigit()),
        "Bluetooth service UUID must contain only hexadecimal digits and hyphens"
    );
    if hex.len() == 32 {
        let bytes = uuid.as_bytes();
        anyhow::ensure!(
            uuid.len() == 36
                && bytes[8] == b'-'
                && bytes[13] == b'-'
                && bytes[18] == b'-'
                && bytes[23] == b'-',
            "128-bit Bluetooth service UUID must use canonical hyphen positions"
        );
    } else {
        anyhow::ensure!(
            !uuid.contains('-'),
            "16-bit and 32-bit Bluetooth service UUIDs cannot contain hyphens"
        );
    }
    Ok(())
}

fn normalize_drop_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn validate_file_association_extension(extension: &str) -> Result<()> {
    anyhow::ensure!(
        !extension.is_empty(),
        "file association extension cannot be empty"
    );
    anyhow::ensure!(
        extension == normalize_drop_extension(extension),
        "file association extension must be normalized"
    );
    anyhow::ensure!(
        extension
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+')),
        "file association extension must contain only ASCII letters, digits, '-', '_', or '+'"
    );
    Ok(())
}

fn sanitize_windows_progid_component(value: &str) -> String {
    let component = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .to_string();

    if component.is_empty() {
        "App".to_string()
    } else {
        component
    }
}

fn classify_file_intake_path(path: &Path, extension: Option<&str>) -> FileIntakeKind {
    if path.is_dir() {
        return FileIntakeKind::Directory;
    }

    match extension.unwrap_or_default() {
        "code-workspace" | "kaelproject" | "kaelproj" => FileIntakeKind::Project,
        "avif" | "bmp" | "gif" | "heic" | "heif" | "jpg" | "jpeg" | "png" | "svg" | "webp" => {
            FileIntakeKind::Image
        }
        "aac" | "aiff" | "flac" | "m4a" | "mp3" | "ogg" | "opus" | "wav" => FileIntakeKind::Audio,
        "avi" | "m4v" | "mkv" | "mov" | "mp4" | "mpeg" | "mpg" | "ogv" | "webm" => {
            FileIntakeKind::Video
        }
        "pdf" => FileIntakeKind::Pdf,
        "css" | "html" | "js" | "jsx" | "log" | "markdown" | "md" | "py" | "rs" | "swift"
        | "ts" | "tsx" | "txt" => FileIntakeKind::Text,
        "csv" | "json" | "jsonl" | "toml" | "yaml" | "yml" => FileIntakeKind::Data,
        "7z" | "br" | "bz2" | "dmg" | "gz" | "pkg" | "rar" | "tar" | "tgz" | "xz" | "zip"
        | "zst" => FileIntakeKind::Archive,
        _ => FileIntakeKind::Unknown,
    }
}

fn system_locale_candidates() -> Vec<(String, Option<String>)> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|value| (value, Some(key.to_string())))
        })
        .collect()
}

fn system_preferred_language_candidates() -> Vec<String> {
    std::env::var("LANGUAGE")
        .ok()
        .map(|value| {
            value
                .split(':')
                .filter(|language| !language.trim().is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_locale_tag(value: &str) -> Result<Option<String>> {
    validate_no_nul(value, "locale tag")?;
    let trimmed = value.trim();
    anyhow::ensure!(
        trimmed == value,
        "locale tag cannot have leading or trailing whitespace"
    );
    if trimmed.is_empty() || matches!(trimmed, "C" | "POSIX") {
        return Ok(None);
    }

    let without_encoding = trimmed.split('.').next().unwrap_or(trimmed);
    let without_modifier = without_encoding
        .split('@')
        .next()
        .unwrap_or(without_encoding);
    let normalized = without_modifier.replace('_', "-");
    let parts = normalized.split('-').collect::<Vec<_>>();
    anyhow::ensure!(!parts.is_empty(), "locale tag cannot be empty");
    anyhow::ensure!(
        parts.iter().all(|part| !part.is_empty()
            && part.len() <= 8
            && part.chars().all(|ch| ch.is_ascii_alphanumeric())),
        "locale tag contains invalid subtags"
    );
    anyhow::ensure!(
        parts[0].len() >= 2 && parts[0].chars().all(|ch| ch.is_ascii_alphabetic()),
        "locale language subtag must be alphabetic"
    );

    let mut output = Vec::with_capacity(parts.len());
    output.push(parts[0].to_ascii_lowercase());
    for (index, part) in parts.iter().enumerate().skip(1) {
        if index == 1
            && (part.len() == 2 || part.len() == 3)
            && part.chars().all(|ch| ch.is_ascii_alphabetic())
        {
            output.push(part.to_ascii_uppercase());
        } else {
            output.push(part.to_ascii_lowercase());
        }
    }

    Ok(Some(output.join("-")))
}

fn locale_region(locale: &str) -> Option<String> {
    locale.split('-').nth(1).and_then(|region| {
        (region.len() == 2 || region.len() == 3).then(|| region.to_ascii_uppercase())
    })
}

fn is_rtl_language(language: &str) -> bool {
    matches!(
        language,
        "ar" | "arc" | "dv" | "fa" | "ha" | "he" | "khw" | "ks" | "ku" | "ps" | "ur" | "yi"
    )
}

fn resolve_app_path_role(app_id: &str, role: AppPathRole) -> Result<PathBuf> {
    let home = home_dir()?;
    match role {
        AppPathRole::Data => app_scoped_base_dir(
            app_id,
            "APPDATA",
            "XDG_DATA_HOME",
            &home,
            &["Library", "Application Support"],
            &["AppData", "Roaming"],
            &[".local", "share"],
        ),
        AppPathRole::Config => app_scoped_base_dir(
            app_id,
            "APPDATA",
            "XDG_CONFIG_HOME",
            &home,
            &["Library", "Application Support"],
            &["AppData", "Roaming"],
            &[".config"],
        ),
        AppPathRole::Cache => {
            let base = if cfg!(target_os = "macos") {
                home_subdir(&home, &["Library", "Caches"])?
            } else if cfg!(target_os = "windows") {
                env_path("LOCALAPPDATA").or(home_subdir(&home, &["AppData", "Local"])?)
            } else {
                env_path("XDG_CACHE_HOME").or(home_subdir(&home, &[".cache"])?)
            }
            .context("failed to resolve cache directory")?;
            Ok(base.join(app_id))
        }
        AppPathRole::Logs => {
            let base = if cfg!(target_os = "macos") {
                home_subdir(&home, &["Library", "Logs"])?
            } else if cfg!(target_os = "windows") {
                env_path("LOCALAPPDATA").or(home_subdir(&home, &["AppData", "Local"])?)
            } else {
                env_path("XDG_STATE_HOME").or(home_subdir(&home, &[".local", "state"])?)
            }
            .context("failed to resolve logs directory")?;
            Ok(base.join(app_id).join("logs"))
        }
        AppPathRole::Temp => Ok(std::env::temp_dir().join(app_id)),
        AppPathRole::Downloads => Ok(home.join("Downloads")),
    }
}

fn app_scoped_base_dir(
    app_id: &str,
    windows_env: &str,
    linux_env: &str,
    home: &Path,
    mac_segments: &[&str],
    windows_segments: &[&str],
    linux_segments: &[&str],
) -> Result<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        home_subdir(home, mac_segments)?
    } else if cfg!(target_os = "windows") {
        env_path(windows_env).or(home_subdir(home, windows_segments)?)
    } else {
        env_path(linux_env).or(home_subdir(home, linux_segments)?)
    }
    .with_context(|| format!("failed to resolve app path base for {app_id}"))?;

    Ok(base.join(app_id))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}

fn home_dir() -> Result<PathBuf> {
    env_path("HOME")
        .or_else(|| env_path("USERPROFILE"))
        .context("failed to resolve home directory")
}

fn home_subdir(home: &Path, segments: &[&str]) -> Result<Option<PathBuf>> {
    anyhow::ensure!(
        !home.as_os_str().is_empty(),
        "home directory cannot be empty"
    );
    Ok(Some(
        segments
            .iter()
            .fold(home.to_path_buf(), |path, segment| path.join(segment)),
    ))
}

fn display_snapshot(
    display: &dyn PlatformDisplay,
    primary_id: Option<DisplayId>,
    cursor_position: Option<Point<Pixels>>,
) -> DisplaySnapshot {
    let bounds = display.bounds();
    DisplaySnapshot {
        id: display.id(),
        uuid: display.uuid().ok(),
        bounds,
        default_bounds: display.default_bounds(),
        refresh_rate: display.refresh_rate(),
        primary: Some(display.id()) == primary_id,
        contains_cursor: cursor_position.is_some_and(|position| bounds.contains(&position)),
    }
}

fn current_process_memory_metrics() -> ProcessMemoryMetrics {
    #[cfg(target_os = "linux")]
    {
        linux_process_memory_metrics().unwrap_or_else(|_| ProcessMemoryMetrics::unsupported())
    }
    #[cfg(target_os = "macos")]
    {
        macos_process_memory_metrics().unwrap_or_else(|_| ProcessMemoryMetrics::unsupported())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        ProcessMemoryMetrics::unsupported()
    }
}

#[cfg(target_os = "linux")]
fn linux_process_memory_metrics() -> std::io::Result<ProcessMemoryMetrics> {
    let statm = fs::read_to_string("/proc/self/statm")?;
    let mut fields = statm.split_whitespace();
    let virtual_pages = fields.next().and_then(|field| field.parse::<u64>().ok());
    let resident_pages = fields.next().and_then(|field| field.parse::<u64>().ok());
    let page_size = linux_page_size();

    Ok(ProcessMemoryMetrics {
        resident_set_bytes: resident_pages.map(|pages| pages.saturating_mul(page_size)),
        virtual_memory_bytes: virtual_pages.map(|pages| pages.saturating_mul(page_size)),
        source: Some("/proc/self/statm"),
    })
}

#[cfg(target_os = "linux")]
fn linux_page_size() -> u64 {
    std::process::Command::new("getconf")
        .arg("PAGESIZE")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|stdout| stdout.trim().parse::<u64>().ok())
        .filter(|page_size| *page_size > 0)
        .unwrap_or(4096)
}

#[cfg(target_os = "macos")]
fn macos_process_memory_metrics() -> std::io::Result<ProcessMemoryMetrics> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=,vsz=", "-p", &std::process::id().to_string()])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ps failed to report process memory",
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.split_whitespace();
    let rss_kib = fields.next().and_then(|field| field.parse::<u64>().ok());
    let vsz_kib = fields.next().and_then(|field| field.parse::<u64>().ok());

    Ok(ProcessMemoryMetrics {
        resident_set_bytes: rss_kib.map(|kib| kib.saturating_mul(1024)),
        virtual_memory_bytes: vsz_kib.map(|kib| kib.saturating_mul(1024)),
        source: Some("ps"),
    })
}

#[cfg(test)]
mod test {
    use std::{
        cell::RefCell,
        fs,
        path::{Path, PathBuf},
        rc::Rc,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[cfg(feature = "media")]
    use crate::MediaKeyEvent;
    #[cfg(feature = "media")]
    use crate::media_playback::{MediaKeyBindingBuilder, VideoController, VideoEvent};
    crate::actions!(jump_list_builder_test, [JumpListOpen]);
    use crate::{
        AppContext, AppDistributionFormat, AppDistributionPlanBuilder, AppDistributionPlatform,
        AppDistributionTargetBuilder, AppIconAssetBuilder, AppIconFormat, AppIconPurpose,
        AppIconSetBuilder, AppLifecycleCommand, AppLifecycleCommandKind, AppLifecyclePolicyBuilder,
        AppMetadataBuilder, AppPackageManifestBuilder, AppPackageReadinessBuilder,
        AppPackageReadinessIssueKind, AppPackageReadinessSeverity, AppPathBuilder, AppPathRole,
        AppPrivacyManifestBuilder, AppPrivacyPermissionBuilder, AppPrivacyPermissionKind,
        AppResourceBudgetBuilder, AppResourceBudgetIssueKind, AppSigningPlanBuilder,
        AppSigningTargetBuilder, AppStorageDurability, AppStorageEntryBuilder, AppStorageKind,
        AppStoragePlanBuilder, AppUpdateAction, AppUpdateChannel, AppUpdateOfferKind,
        AppUpdateOfferPolicyBuilder, AppUpdateOfferReason, AppUpdatePhase, AppUpdateReleaseBuilder,
        AppUpdateStateBuilder, AppWindowCaptureFormat, AppWindowCaptureRequest,
        AppWindowCaptureRequestBuilder, AppWindowCaptureTarget, AttentionType, AutoLaunchBuilder,
        BiometricAuthBuilder, BiometricKind, BiometricStatus, Capability, CredentialBuilder,
        CredentialServiceBuilder, CustomProtocolFileResolver, CustomProtocolRequest,
        CustomProtocolResponse, CustomProtocolResponseBuilder, CustomProtocolRouterBuilder,
        DeepLinkHandler, DeepLinkRouterBuilder, DefaultHandlerPlanBuilder, DefaultHandlerScope,
        DeviceAccessKind, DeviceAccessRequest, DeviceAccessRequestBuilder, DisplayQueryBuilder,
        DockBadgeBuilder, FileAssociationBuilder, FileAssociationRole, FileAssociationSetBuilder,
        FileDropIntentBuilder, FileDropPathKind, FileDropPurpose, FileExportDragIntentBuilder,
        FileExportDragItem, FileIconRequestBuilder, FileIconSize, FileIntakeKind,
        FileIntakePlanBuilder, JumpListBuilder, LaunchContextBuilder, LocaleSnapshotBuilder,
        LocaleTextDirection, LocationAccuracy, LocationRequest, LocationRequestBuilder, MenuItem,
        NativeThemeSnapshot, NetworkStatus, NetworkStatusMonitorBuilder, OpenRequestKind,
        PathScope, PermissionRequestBuilder, PermissionStatus, PowerMode, PowerSaveBlockerBuilder,
        PowerSaveBlockerKind, ProcessId, RecentDocumentsBuilder, RestartPathBuilder,
        SHUTDOWN_TIMEOUT, ShellTarget, ShellTargetsBuilder, SupportDiagnosticsBuilder,
        SystemIdleEvaluation, SystemIdlePolicyBuilder, SystemPowerEvent, SystemPowerMonitorBuilder,
        SystemPowerSnapshot, TestAppContext, TextCheckingRequest, TextCheckingRequestBuilder,
        TrashRequestBuilder, TrayAppBuilder, TrayTooltipBuilder, UrlSchemeRegistrationBuilder,
        UserAttentionBuilder, WindowAppearance, WindowBounds, WindowOptionsBuilder,
        WindowPlacementBuilder, px, size,
    };
    #[cfg(feature = "share")]
    use crate::{ShareSheetBuilder, ShareType};

    struct TestDeepLinkHandler {
        scheme: &'static str,
        hits: Arc<Mutex<Vec<String>>>,
    }

    impl DeepLinkHandler for TestDeepLinkHandler {
        fn scheme(&self) -> &str {
            self.scheme
        }

        fn handle(&self, url: &str) {
            self.hits.lock().unwrap().push(url.to_string());
        }
    }

    #[test]
    fn test_gpui_borrow() {
        let cx = TestAppContext::single();
        let observation_count = Rc::new(RefCell::new(0));

        let state = cx.update(|cx| {
            let state = cx.new(|_| false);
            cx.observe(&state, {
                let observation_count = observation_count.clone();
                move |_, _| {
                    let mut count = observation_count.borrow_mut();
                    *count += 1;
                }
            })
            .detach();

            state
        });

        cx.update(|cx| {
            // Calling this like this so that we don't clobber the borrow_mut above
            *std::borrow::BorrowMut::borrow_mut(&mut state.as_mut(cx)) = true;
        });

        cx.update(|cx| {
            state.write(cx, false);
        });

        assert_eq!(*observation_count.borrow(), 2);
    }

    #[test]
    fn test_open_url_dispatch_multiplexes_observers_and_deep_links() {
        let cx = TestAppContext::single();
        let batch_urls = Rc::new(RefCell::new(Vec::new()));
        let single_urls = Rc::new(RefCell::new(Vec::new()));
        let batch_requests = Rc::new(RefCell::new(Vec::new()));
        let single_requests = Rc::new(RefCell::new(Vec::new()));
        let deep_link_urls = Rc::new(RefCell::new(Vec::new()));
        let runtime_hits = Arc::new(Mutex::new(Vec::new()));

        cx.app.on_open_urls({
            let batch_urls = batch_urls.clone();
            move |urls| {
                batch_urls.borrow_mut().push(urls);
            }
        });

        cx.app.on_open_url({
            let single_urls = single_urls.clone();
            move |url, _| {
                single_urls.borrow_mut().push(url);
            }
        });

        cx.app.on_open_requests({
            let batch_requests = batch_requests.clone();
            move |requests| {
                batch_requests.borrow_mut().push(requests);
            }
        });

        cx.app.on_open_request({
            let single_requests = single_requests.clone();
            move |request, _| {
                single_requests.borrow_mut().push(request);
            }
        });

        cx.app.on_deep_link("myapp", {
            let deep_link_urls = deep_link_urls.clone();
            move |url, _| {
                deep_link_urls.borrow_mut().push(url);
            }
        });

        let route_urls = Rc::new(RefCell::new(Vec::new()));
        cx.app
            .deep_links_checked(DeepLinkRouterBuilder::new().route("myapp", {
                let route_urls = route_urls.clone();
                move |url, _| {
                    route_urls.borrow_mut().push(url);
                }
            }))
            .unwrap();

        cx.update(|app| {
            app.register_deep_link_handler(Box::new(TestDeepLinkHandler {
                scheme: "myapp",
                hits: runtime_hits.clone(),
            }));
        });

        cx.simulate_open_urls(&[
            "myapp://settings",
            "https://example.com/help",
            "file:///tmp/Project%20Plan.kael",
        ]);

        assert_eq!(
            batch_urls.borrow().clone(),
            vec![vec![
                "myapp://settings".to_string(),
                "https://example.com/help".to_string(),
                "file:///tmp/Project%20Plan.kael".to_string(),
            ]]
        );
        assert_eq!(
            single_urls.borrow().clone(),
            vec![
                "myapp://settings".to_string(),
                "https://example.com/help".to_string(),
                "file:///tmp/Project%20Plan.kael".to_string(),
            ]
        );
        assert_eq!(batch_requests.borrow().len(), 1);
        assert_eq!(
            batch_requests.borrow()[0]
                .iter()
                .map(|request| request.kind().clone())
                .collect::<Vec<_>>(),
            vec![
                OpenRequestKind::DeepLink {
                    scheme: "myapp".to_string()
                },
                OpenRequestKind::Url {
                    scheme: "https".to_string()
                },
                OpenRequestKind::File {
                    path: PathBuf::from("/tmp/Project Plan.kael")
                },
            ]
        );
        assert_eq!(
            single_requests
                .borrow()
                .iter()
                .filter_map(|request| request.scheme().map(str::to_string))
                .collect::<Vec<_>>(),
            vec!["myapp", "https", "file"]
        );
        assert_eq!(
            single_requests.borrow()[2].file_path(),
            Some(std::path::Path::new("/tmp/Project Plan.kael"))
        );
        assert_eq!(
            deep_link_urls.borrow().clone(),
            vec!["myapp://settings".to_string()]
        );
        assert_eq!(
            route_urls.borrow().clone(),
            vec!["myapp://settings".to_string()]
        );
        assert_eq!(
            runtime_hits.lock().unwrap().clone(),
            vec!["myapp://settings".to_string()]
        );
    }

    #[test]
    fn deep_link_router_builder_validates_routes() {
        assert!(DeepLinkRouterBuilder::new().validate().is_err());
        assert!(
            DeepLinkRouterBuilder::new()
                .route("1bad", |_, _| {})
                .validate()
                .is_err()
        );
        assert!(
            DeepLinkRouterBuilder::new()
                .route("myapp", |_, _| {})
                .route("myapp", |_, _| {})
                .validate()
                .is_err()
        );

        let cx = TestAppContext::single();
        assert!(
            cx.app
                .deep_links_checked(DeepLinkRouterBuilder::new().route("bad scheme", |_, _| {}))
                .is_err()
        );
    }

    #[test]
    fn custom_protocol_request_parses_typed_parts() {
        let request = CustomProtocolRequest::parse("app://assets/icons/logo.svg?v=1").unwrap();
        assert_eq!(request.raw_url(), "app://assets/icons/logo.svg?v=1");
        assert_eq!(request.scheme(), "app");
        assert_eq!(request.host(), Some("assets"));
        assert_eq!(request.path(), "/icons/logo.svg");
        assert_eq!(request.query(), Some("v=1"));

        assert!(CustomProtocolRequest::parse(" https://example.com").is_err());
        assert!(CustomProtocolRequest::parse("https://example.com").is_err());
        assert!(CustomProtocolRequest::parse("javascript:alert(1)").is_err());
    }

    #[test]
    fn custom_protocol_response_builder_validates_metadata() {
        let response = CustomProtocolResponse::html("<h1>Hello</h1>").unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.mime_type, "text/html; charset=utf-8");

        assert!(
            CustomProtocolResponseBuilder::new("text/plain")
                .status(99)
                .build_checked()
                .is_err()
        );
        assert!(
            CustomProtocolResponseBuilder::new("plain")
                .build_checked()
                .is_err()
        );
        assert!(
            CustomProtocolResponseBuilder::new("text/plain")
                .header("Bad Header", "value")
                .build_checked()
                .is_err()
        );
        assert!(
            CustomProtocolResponseBuilder::new("text/plain")
                .header("X-Test", "bad\nvalue")
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn custom_protocol_file_resolver_serves_checked_files() {
        let root =
            std::env::temp_dir().join(format!("kael_custom_protocol_files_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("icons")).unwrap();
        fs::write(root.join("index.html"), "<main>Home</main>").unwrap();
        fs::write(root.join("icons").join("logo.svg"), "<svg></svg>").unwrap();

        let resolver = CustomProtocolFileResolver::builder(&root)
            .host("assets")
            .index_file("index.html")
            .cache_control("public, max-age=60")
            .require_existing_root()
            .canonicalize_root()
            .build_checked()
            .unwrap();

        let request = CustomProtocolRequest::parse("app://assets/icons/logo.svg").unwrap();
        let response = resolver.response(&request).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.mime_type, "image/svg+xml");
        assert_eq!(response.body, b"<svg></svg>".to_vec());
        assert_eq!(
            response.headers,
            vec![(
                "Cache-Control".to_string(),
                "public, max-age=60".to_string()
            )]
        );

        let index = resolver
            .response(&CustomProtocolRequest::parse("app://assets/").unwrap())
            .unwrap();
        assert_eq!(index.mime_type, "text/html; charset=utf-8");
        assert_eq!(index.body, b"<main>Home</main>".to_vec());

        let wrong_host = resolver
            .response(&CustomProtocolRequest::parse("app://other/icons/logo.svg").unwrap())
            .unwrap();
        assert_eq!(wrong_host.status, 404);

        let missing = resolver
            .response(&CustomProtocolRequest::parse("app://assets/missing.txt").unwrap())
            .unwrap();
        assert_eq!(missing.status, 404);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_protocol_file_resolver_rejects_unsafe_paths() {
        let root = std::env::temp_dir().join(format!(
            "kael_custom_protocol_escape_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let resolver = CustomProtocolFileResolver::builder(&root)
            .require_existing_root()
            .build_checked()
            .unwrap();

        assert!(
            resolver
                .resolve_path(&CustomProtocolRequest::parse("app://assets/../secret.txt").unwrap())
                .is_err()
        );
        assert!(
            resolver
                .resolve_path(
                    &CustomProtocolRequest::parse("app://assets/%2e%2e/secret.txt").unwrap()
                )
                .is_err()
        );
        assert!(
            CustomProtocolFileResolver::builder("")
                .build_checked()
                .is_err()
        );
        assert!(
            CustomProtocolFileResolver::builder(&root)
                .host("bad host")
                .validate()
                .is_err()
        );
        assert!(
            CustomProtocolFileResolver::builder(&root)
                .index_file("../index.html")
                .validate()
                .is_err()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_protocol_router_builder_validates_routes() {
        assert!(CustomProtocolRouterBuilder::new().validate().is_err());
        assert!(
            CustomProtocolRouterBuilder::new()
                .route("1bad", |_, _| Ok(CustomProtocolResponse::not_found()))
                .validate()
                .is_err()
        );
        assert!(
            CustomProtocolRouterBuilder::new()
                .route("https", |_, _| Ok(CustomProtocolResponse::not_found()))
                .validate()
                .is_err()
        );
        assert!(
            CustomProtocolRouterBuilder::new()
                .route("app", |_, _| Ok(CustomProtocolResponse::not_found()))
                .route("app", |_, _| Ok(CustomProtocolResponse::not_found()))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn custom_protocol_routes_can_be_registered_and_invoked() {
        let cx = TestAppContext::single();

        cx.app
            .custom_protocols_checked(CustomProtocolRouterBuilder::new().route(
                "app",
                |request, _| {
                    CustomProtocolResponseBuilder::new("text/plain")
                        .header("X-App-Path", request.path())
                        .body(format!("asset:{}", request.path()).into_bytes())
                        .build_checked()
                },
            ))
            .unwrap();

        cx.update(|app| {
            assert!(app.has_custom_protocol("app"));
            assert_eq!(app.custom_protocol_schemes(), vec!["app".to_string()]);

            let response = app
                .handle_custom_protocol_url("app://assets/readme.txt")
                .unwrap()
                .unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(
                response.headers[0],
                ("X-App-Path".to_string(), "/readme.txt".to_string())
            );
            assert_eq!(response.body, b"asset:/readme.txt".to_vec());

            assert!(
                app.handle_custom_protocol_url("missing://asset")
                    .unwrap()
                    .is_none()
            );
            assert!(
                app.handle_custom_protocol_url("https://example.com")
                    .is_err()
            );
        });
    }

    #[test]
    fn custom_protocol_file_resolver_can_register_route() {
        let cx = TestAppContext::single();
        let root =
            std::env::temp_dir().join(format!("kael_custom_protocol_route_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("readme.txt"), "hello").unwrap();

        let route = CustomProtocolFileResolver::builder(&root)
            .host("assets")
            .require_existing_root()
            .route_checked("app")
            .unwrap();

        cx.app
            .custom_protocols_checked(CustomProtocolRouterBuilder::from(route))
            .unwrap();

        cx.update(|app| {
            let response = app
                .handle_custom_protocol_url("app://assets/readme.txt")
                .unwrap()
                .unwrap();
            assert_eq!(response.mime_type, "text/plain; charset=utf-8");
            assert_eq!(response.body, b"hello".to_vec());
        });

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(feature = "share")]
    #[test]
    fn share_sheet_builder_is_available_from_app() {
        let cx = TestAppContext::single();

        cx.update(|app| {
            let _support = app.share_support();

            let sheet = ShareSheetBuilder::new()
                .subject("Build report")
                .text("All checks passed")
                .url("https://example.com/report")
                .exclude(ShareType::Social)
                .build_checked()
                .unwrap();
            sheet.validate().unwrap();
        });
    }

    #[test]
    fn open_external_url_records_url_on_test_platform() {
        let cx = TestAppContext::single();

        cx.update(|app| {
            app.open_external_url("https://example.com/docs").unwrap();
            app.open_shell_target(ShellTarget::url("https://example.com/help"))
                .unwrap();
            assert!(app.open_external_url("ftp://example.com/file").is_err());
            assert!(app.open_external_url(" https://example.com").is_err());
            assert!(app.open_path("").is_err());
        });

        assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/help"));
    }

    #[test]
    fn shell_targets_builder_validates_and_batches_targets() {
        let executable = std::env::current_exe().unwrap();
        let targets = ShellTargetsBuilder::new()
            .url("https://example.com/docs")
            .reveal_path(&executable)
            .require_existing_paths();

        assert_eq!(
            targets.configured_targets(),
            &[
                ShellTarget::url("https://example.com/docs"),
                ShellTarget::reveal_path(&executable)
            ]
        );
        assert!(targets.validate().is_ok());
        assert!(ShellTargetsBuilder::new().validate().is_err());
        assert!(
            ShellTargetsBuilder::new()
                .url("javascript:alert(1)")
                .validate()
                .is_err()
        );
        assert!(
            ShellTargetsBuilder::new()
                .url(" https://example.com")
                .validate()
                .is_err()
        );
        assert!(ShellTargetsBuilder::new().path("").validate().is_err());
        assert!(
            ShellTargetsBuilder::new()
                .path("/definitely/not/a/path")
                .require_existing_paths()
                .validate()
                .is_err()
        );
        let built = ShellTargetsBuilder::new()
            .path(&executable)
            .canonicalize_paths()
            .build()
            .unwrap();
        assert_eq!(
            built,
            vec![ShellTarget::path(executable.canonicalize().unwrap())]
        );
    }

    #[test]
    fn open_shell_targets_routes_batch_to_platform() {
        let cx = TestAppContext::single();

        let opened = cx.update(|app| {
            app.open_shell_targets(
                ShellTargetsBuilder::new()
                    .url("https://example.com/docs")
                    .url("https://example.com/help"),
            )
            .unwrap()
        });

        assert_eq!(
            opened,
            vec![
                ShellTarget::url("https://example.com/docs"),
                ShellTarget::url("https://example.com/help")
            ]
        );
        assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/help"));
    }

    #[test]
    fn trash_request_builder_validates_safe_trash_targets() {
        let temp = std::env::temp_dir().join(format!(
            "kael_trash_request_{}_absolute.tmp",
            std::process::id()
        ));
        let relative = PathBuf::from(format!(
            "target/kael_trash_request_{}_relative.tmp",
            std::process::id()
        ));
        let _ = fs::remove_file(&temp);
        let _ = fs::remove_file(&relative);
        fs::create_dir_all(relative.parent().unwrap()).unwrap();
        fs::write(&temp, "trash me").unwrap();
        fs::write(&relative, "trash me too").unwrap();

        let request = TrashRequestBuilder::new(&temp)
            .canonicalize_path()
            .build_checked()
            .unwrap();

        assert_eq!(request.path(), temp.canonicalize().unwrap().as_path());
        assert!(request.requires_existing_path());
        assert!(request.is_canonicalized());
        assert!(!request.allows_relative_path());

        let relative_request = TrashRequestBuilder::new(&relative)
            .allow_relative_path()
            .build_checked()
            .unwrap();

        assert_eq!(relative_request.path(), relative.as_path());
        assert!(relative_request.requires_existing_path());
        assert!(!relative_request.is_canonicalized());
        assert!(relative_request.allows_relative_path());

        let _ = fs::remove_file(&temp);
        let _ = fs::remove_file(&relative);
    }

    #[test]
    fn trash_request_builder_rejects_generated_footguns() {
        let missing = std::env::temp_dir().join(format!(
            "kael_trash_request_{}_missing.tmp",
            std::process::id()
        ));
        let _ = fs::remove_file(&missing);

        assert!(TrashRequestBuilder::new("").validate().is_err());
        assert!(TrashRequestBuilder::new("bad\0path").validate().is_err());
        assert!(TrashRequestBuilder::new("relative.txt").validate().is_err());
        assert!(TrashRequestBuilder::new(Path::new("/")).validate().is_err());
        assert!(TrashRequestBuilder::new(missing).validate().is_err());
    }

    #[test]
    fn trash_request_checked_uses_builder_validation_and_capability() {
        let cx = TestAppContext::single();
        let mut broker = cx.read(|app| app.permission_broker().clone());
        let process_id = cx.read(|app| app.current_process_id());
        broker.grant(process_id, Capability::ShellExecute);
        cx.update(|app| app.set_permission_broker(broker));

        let temp = std::env::temp_dir().join(format!(
            "kael_trash_request_{}_checked.tmp",
            std::process::id()
        ));
        let _ = fs::remove_file(&temp);
        fs::write(&temp, "trash me after validation").unwrap();

        let request = cx
            .read(|app| app.trash_request_checked(TrashRequestBuilder::new(&temp)))
            .unwrap();

        assert_eq!(request.path(), temp.as_path());
        assert!(
            cx.read(|app| app.trash_request_checked(TrashRequestBuilder::new("")))
                .is_err()
        );

        let _ = fs::remove_file(&temp);
    }

    #[test]
    fn url_scheme_registration_builder_validates_and_dedupes() {
        let registration = UrlSchemeRegistrationBuilder::new()
            .scheme("myapp")
            .scheme("myapp")
            .schemes(["myapp-beta", "myapp.dev"]);

        assert_eq!(
            registration.configured_schemes(),
            &[
                "myapp".to_string(),
                "myapp-beta".to_string(),
                "myapp.dev".to_string()
            ]
        );
        assert!(registration.validate().is_ok());

        assert!(UrlSchemeRegistrationBuilder::new().validate().is_err());
        assert!(
            UrlSchemeRegistrationBuilder::new()
                .scheme("1bad")
                .validate()
                .is_err()
        );
        assert!(
            UrlSchemeRegistrationBuilder::new()
                .scheme("bad scheme")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn register_url_schemes_uses_builder_options() {
        let cx = TestAppContext::single();

        let tasks = cx
            .read(|app| {
                app.register_url_schemes(
                    UrlSchemeRegistrationBuilder::new().schemes(["myapp", "myapp-auth"]),
                )
            })
            .unwrap();

        for task in tasks {
            cx.background_executor.block(task).unwrap();
        }

        assert_eq!(
            cx.registered_url_schemes(),
            vec!["myapp".to_string(), "myapp-auth".to_string()]
        );
    }

    #[test]
    fn file_association_builder_validates_document_types() {
        let association = FileAssociationBuilder::new("Markdown")
            .extensions([".md", "markdown", "MD"])
            .mime_type("text/markdown")
            .editor()
            .description("Markdown documents")
            .build_checked()
            .unwrap();

        assert_eq!(association.name(), "Markdown");
        assert_eq!(
            association.extensions(),
            &["md".to_string(), "markdown".to_string()]
        );
        assert_eq!(association.mime_types(), &["text/markdown".to_string()]);
        assert_eq!(association.role(), FileAssociationRole::Editor);
        assert_eq!(association.description(), Some("Markdown documents"));

        assert!(
            FileAssociationBuilder::new("")
                .extension("md")
                .validate()
                .is_err()
        );
        assert!(FileAssociationBuilder::new("Empty").validate().is_err());
        assert!(
            FileAssociationBuilder::new("Bad")
                .extension("bad ext")
                .validate()
                .is_err()
        );
        assert!(
            FileAssociationBuilder::new("Bad")
                .mime_type("not a mime")
                .validate()
                .is_err()
        );
        assert!(
            FileAssociationBuilder::new("Bad")
                .description(" bad")
                .extension("bad")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn file_association_set_rejects_duplicate_claims() {
        let set = FileAssociationSetBuilder::new()
            .association(
                FileAssociationBuilder::new("Markdown")
                    .extension("md")
                    .mime_type("text/markdown")
                    .editor(),
            )
            .association(
                FileAssociationBuilder::new("PDF")
                    .extension("pdf")
                    .mime_type("application/pdf"),
            )
            .build_checked()
            .unwrap();

        assert_eq!(set.associations().len(), 2);
        assert!(set.accepts_extension(".MD"));
        assert!(set.accepts_mime_type("APPLICATION/PDF"));
        assert!(!set.accepts_extension("txt"));
        assert!(FileAssociationSetBuilder::new().validate().is_err());
        assert!(
            FileAssociationSetBuilder::new()
                .association(FileAssociationBuilder::new("One").extension("md"))
                .association(FileAssociationBuilder::new("Two").extension(".MD"))
                .validate()
                .is_err()
        );
        assert!(
            FileAssociationSetBuilder::new()
                .association(FileAssociationBuilder::new("One").mime_type("text/markdown"))
                .association(FileAssociationBuilder::new("Two").mime_type("TEXT/MARKDOWN"))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn file_associations_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let set = cx
            .read(|app| {
                app.file_associations_checked(
                    FileAssociationSetBuilder::new().association(
                        FileAssociationBuilder::new("Kael Project")
                            .extension("kaelproj")
                            .mime_type("application/x-kael-project")
                            .editor(),
                    ),
                )
            })
            .unwrap();

        assert!(set.accepts_extension("kaelproj"));
        assert!(
            cx.read(|app| app.file_associations_checked(FileAssociationSetBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn default_handler_plan_builder_validates_runtime_claims() {
        let plan = DefaultHandlerPlanBuilder::new("com.example.kael")
            .app_name("Kael Studio")
            .schemes(["kael", "kael-auth"])
            .file_association(
                FileAssociationBuilder::new("Kael Project")
                    .extension("kaelproj")
                    .mime_type("application/x-kael-project")
                    .editor(),
            )
            .system_scope()
            .require_user_confirmation(true)
            .build_checked()
            .unwrap();

        assert_eq!(plan.app_id(), "com.example.kael");
        assert_eq!(plan.app_name(), Some("Kael Studio"));
        assert_eq!(plan.scope(), DefaultHandlerScope::System);
        assert!(plan.requires_user_confirmation());
        assert!(plan.claims_scheme("KAEL"));
        assert!(plan.claims_extension(".KAELPROJ"));
        assert!(plan.claims_mime_type("APPLICATION/X-KAEL-PROJECT"));
        assert!(!plan.claims_scheme("mailto"));
    }

    #[test]
    fn default_handler_plan_can_be_seeded_from_package_manifest() {
        let manifest = AppPackageManifestBuilder::new(
            AppMetadataBuilder::new("Kael Studio").identifier("com.example.kael"),
        )
        .url_schemes(UrlSchemeRegistrationBuilder::new().schemes(["kael", "kael-auth"]))
        .file_associations(
            FileAssociationSetBuilder::new().association(
                FileAssociationBuilder::new("Kael Project")
                    .extension("kaelproj")
                    .mime_type("application/x-kael-project")
                    .editor(),
            ),
        )
        .build_checked()
        .unwrap();

        let plan = DefaultHandlerPlanBuilder::from_package_manifest(&manifest)
            .current_user_scope()
            .build_checked()
            .unwrap();

        assert_eq!(plan.app_id(), "com.example.kael");
        assert_eq!(plan.app_name(), Some("Kael Studio"));
        assert_eq!(
            plan.url_schemes(),
            &["kael".to_string(), "kael-auth".to_string()]
        );
        assert_eq!(plan.file_associations().len(), 1);
        assert!(plan.claims_extension("kaelproj"));
    }

    #[test]
    fn default_handler_plan_builder_rejects_unsafe_claims() {
        assert!(
            DefaultHandlerPlanBuilder::new("")
                .scheme("kael")
                .validate()
                .is_err()
        );
        assert!(
            DefaultHandlerPlanBuilder::new("com.example.kael")
                .validate()
                .is_err()
        );
        assert!(
            DefaultHandlerPlanBuilder::new("com.example.kael")
                .scheme("bad scheme")
                .validate()
                .is_err()
        );
        assert!(
            DefaultHandlerPlanBuilder::new("com.example.kael")
                .schemes(["kael", "KAEL"])
                .validate()
                .is_err()
        );
        assert!(
            DefaultHandlerPlanBuilder::new("com.example.kael")
                .app_name(" Kael")
                .scheme("kael")
                .validate()
                .is_err()
        );
        assert!(
            DefaultHandlerPlanBuilder::new("com.example.kael")
                .file_association(FileAssociationBuilder::new("Bad").extension("bad ext"))
                .validate()
                .is_err()
        );
        assert!(
            DefaultHandlerPlanBuilder::new("com.example.kael")
                .file_association(FileAssociationBuilder::new("One").extension("kaelproj"))
                .file_association(FileAssociationBuilder::new("Two").extension(".KAELPROJ"))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn default_handler_plan_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let plan = cx
            .read(|app| {
                app.default_handler_plan_checked(
                    DefaultHandlerPlanBuilder::new("com.example.kael").scheme("kael"),
                )
            })
            .unwrap();

        assert!(plan.claims_scheme("kael"));
        assert!(
            cx.read(|app| {
                app.default_handler_plan_checked(DefaultHandlerPlanBuilder::new("com.example.kael"))
            })
            .is_err()
        );
    }

    #[test]
    fn app_icon_set_builder_validates_icon_assets() {
        let icons = AppIconSetBuilder::new()
            .icon(AppIconAssetBuilder::app("assets/icon-512.png").size_px(512))
            .icon(AppIconAssetBuilder::tray("assets/tray.svg").template())
            .icon(
                AppIconAssetBuilder::installer("assets/install")
                    .format(AppIconFormat::Ico)
                    .size_px(256),
            )
            .build_checked()
            .unwrap();

        assert_eq!(icons.icons().len(), 3);
        assert_eq!(icons.icons()[0].format(), AppIconFormat::Png);
        assert_eq!(icons.icons()[0].size_px(), Some(512));
        assert!(icons.icons()[1].is_template());
        assert_eq!(AppIconFormat::Svg.mime_type(), "image/svg+xml");
        assert_eq!(AppIconFormat::Icns.extension(), "icns");
        assert_eq!(icons.icons_for(AppIconPurpose::Tray).len(), 1);

        assert!(AppIconSetBuilder::new().validate().is_err());
        assert!(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app(""))
                .validate()
                .is_err()
        );
        assert!(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app("assets/icon.bmp"))
                .validate()
                .is_err()
        );
        assert!(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app("assets/vector.svg").size_px(32))
                .validate()
                .is_err()
        );
        assert!(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app("assets/missing.png").require_existing_path())
                .validate()
                .is_err()
        );
        assert!(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app("assets/icon.png").size_px(64))
                .icon(AppIconAssetBuilder::app("assets/icon.png").size_px(64))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_icons_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let icons = cx
            .read(|app| {
                app.app_icons_checked(
                    AppIconSetBuilder::new()
                        .icon(AppIconAssetBuilder::app("assets/app.icns"))
                        .icon(AppIconAssetBuilder::document("assets/document.png").size_px(128)),
                )
            })
            .unwrap();

        assert_eq!(icons.icons_for(AppIconPurpose::Document).len(), 1);
        assert!(
            cx.read(|app| app.app_icons_checked(AppIconSetBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn file_icon_request_builder_validates_native_icon_requests() {
        let existing = std::env::current_exe().unwrap();
        let request = FileIconRequestBuilder::new(&existing)
            .large()
            .require_existing_path()
            .allow_generic_fallback(false)
            .build_checked()
            .unwrap();

        assert_eq!(request.path(), existing.as_path());
        assert_eq!(request.size(), FileIconSize::Large);
        assert_eq!(request.size().pixels(), 64);
        assert_eq!(request.size().key(), "large");
        assert!(request.requires_existing_path());
        assert!(!request.allows_generic_fallback());

        let planned = FileIconRequestBuilder::new("Report.KAELPROJ")
            .small()
            .build_checked()
            .unwrap();
        assert_eq!(planned.size(), FileIconSize::Small);
        assert_eq!(planned.extension_hint(), Some("kaelproj".to_string()));

        let custom = FileIconRequestBuilder::new("Design.fig")
            .custom_size_px(128)
            .build_checked()
            .unwrap();
        assert_eq!(custom.size(), FileIconSize::Custom(128));
        assert_eq!(custom.size().key(), "custom");
    }

    #[test]
    fn file_icon_request_builder_rejects_generated_footguns() {
        assert!(FileIconRequestBuilder::new("").validate().is_err());
        assert!(
            FileIconRequestBuilder::new("Report\0.md")
                .validate()
                .is_err()
        );
        assert!(FileIconRequestBuilder::new("missing").validate().is_err());
        assert!(
            FileIconRequestBuilder::new("missing.md")
                .allow_generic_fallback(false)
                .validate()
                .is_err()
        );
        assert!(
            FileIconRequestBuilder::new("missing.md")
                .require_existing_path()
                .validate()
                .is_err()
        );
        assert!(
            FileIconRequestBuilder::new("missing.md")
                .custom_size_px(0)
                .validate()
                .is_err()
        );
        assert!(
            FileIconRequestBuilder::new("missing.md")
                .custom_size_px(1025)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn file_icon_request_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let request = cx
            .read(|app| {
                app.file_icon_request_checked(
                    FileIconRequestBuilder::new("Project.kaelproj").normal(),
                )
            })
            .unwrap();

        assert_eq!(request.size(), FileIconSize::Normal);
        assert_eq!(request.size().pixels(), 32);
        assert_eq!(request.extension_hint(), Some("kaelproj".to_string()));
        assert!(
            cx.read(|app| app.file_icon_request_checked(FileIconRequestBuilder::new("missing")))
                .is_err()
        );
    }

    #[test]
    fn privacy_manifest_builder_validates_permissions() {
        let manifest = AppPrivacyManifestBuilder::new()
            .permission(AppPrivacyPermissionBuilder::camera(
                "Camera access lets you record video notes.",
            ))
            .permission(AppPrivacyPermissionBuilder::microphone(
                "Microphone access lets you record narration.",
            ))
            .permission(AppPrivacyPermissionBuilder::screen_capture(
                "Screen capture lets you share your workspace.",
            ))
            .build_checked()
            .unwrap();

        assert!(manifest.declares(AppPrivacyPermissionKind::Camera));
        assert!(manifest.declares(AppPrivacyPermissionKind::ScreenCapture));
        let usage = manifest.macos_usage_descriptions();
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].key(), "NSCameraUsageDescription");
        assert_eq!(usage[1].key(), "NSMicrophoneUsageDescription");

        let network = AppPrivacyPermissionBuilder::from_capability(
            &Capability::Network {
                hosts: vec!["api.example.com".to_string()],
            },
            "Network access syncs project data.",
        )
        .unwrap()
        .build_checked()
        .unwrap();
        assert_eq!(network.kind(), AppPrivacyPermissionKind::Network);

        assert!(AppPrivacyManifestBuilder::new().validate().is_err());
        assert!(
            AppPrivacyManifestBuilder::new()
                .permission(AppPrivacyPermissionBuilder::camera(""))
                .validate()
                .is_err()
        );
        assert!(
            AppPrivacyManifestBuilder::new()
                .permission(AppPrivacyPermissionBuilder::camera("Use the camera."))
                .permission(AppPrivacyPermissionBuilder::camera("Use the camera again."))
                .validate()
                .is_err()
        );
        assert!(
            AppPrivacyPermissionBuilder::from_capability(
                &Capability::ClipboardRead,
                "Read clipboard data.",
            )
            .is_none()
        );
    }

    #[test]
    fn privacy_manifest_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let manifest = cx
            .read(|app| {
                app.privacy_manifest_checked(AppPrivacyManifestBuilder::new().permission(
                    AppPrivacyPermissionBuilder::location("Location finds nearby projects."),
                ))
            })
            .unwrap();

        assert_eq!(
            manifest.macos_usage_descriptions()[0].key(),
            "NSLocationWhenInUseUsageDescription"
        );
        assert!(
            cx.read(|app| app.privacy_manifest_checked(AppPrivacyManifestBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn package_manifest_exports_platform_declarations() {
        let manifest = AppPackageManifestBuilder::new(
            AppMetadataBuilder::new("Kael Studio")
                .identifier("com.example.kael-studio")
                .version("1.2.3"),
        )
        .url_schemes(UrlSchemeRegistrationBuilder::new().schemes(["kael", "kael-auth"]))
        .file_associations(
            FileAssociationSetBuilder::new()
                .association(
                    FileAssociationBuilder::new("Markdown")
                        .extensions(["md", "markdown"])
                        .mime_type("text/markdown")
                        .editor(),
                )
                .association(
                    FileAssociationBuilder::new("Kael Project")
                        .extension("kaelproj")
                        .mime_type("application/x-kael-project")
                        .description("Kael project file"),
                ),
        )
        .icons(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app("assets/app.icns"))
                .icon(AppIconAssetBuilder::tray("assets/tray.svg").template()),
        )
        .privacy_permissions(
            AppPrivacyManifestBuilder::new()
                .permission(AppPrivacyPermissionBuilder::camera(
                    "Camera access records video notes.",
                ))
                .permission(AppPrivacyPermissionBuilder::microphone(
                    "Microphone access records narration.",
                )),
        )
        .build_checked()
        .unwrap();

        assert_eq!(
            manifest.metadata().identifier(),
            Some("com.example.kael-studio")
        );
        assert_eq!(
            manifest.url_schemes(),
            &["kael".to_string(), "kael-auth".to_string()]
        );
        assert!(manifest.accepts_extension(".MD"));
        assert!(manifest.accepts_mime_type("APPLICATION/X-KAEL-PROJECT"));

        let url_types = manifest.macos_url_types();
        assert_eq!(url_types[0].name(), "com.example.kael-studio");
        assert_eq!(
            url_types[0].schemes(),
            &["kael".to_string(), "kael-auth".to_string()]
        );

        let document_types = manifest.macos_document_types();
        assert_eq!(document_types[0].name(), "Markdown");
        assert_eq!(document_types[0].role(), FileAssociationRole::Editor);
        assert_eq!(
            manifest.linux_desktop_mime_types(),
            vec![
                "text/markdown".to_string(),
                "application/x-kael-project".to_string()
            ]
        );

        let windows = manifest.windows_file_associations();
        assert_eq!(windows[0].prog_id(), "com.example.kael_studio.Markdown");
        assert_eq!(windows[1].description(), Some("Kael project file"));
        assert_eq!(manifest.icons().len(), 2);
        assert_eq!(manifest.icons_for(AppIconPurpose::Tray).len(), 1);
        assert_eq!(manifest.privacy_permissions().len(), 2);
        assert_eq!(
            manifest.macos_usage_descriptions()[0].key(),
            "NSCameraUsageDescription"
        );
    }

    #[test]
    fn package_manifest_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let manifest = cx
            .read(|app| {
                app.package_manifest_checked(
                    AppPackageManifestBuilder::new(
                        AppMetadataBuilder::new("Kael Studio").identifier("com.example.kael"),
                    )
                    .url_schemes(UrlSchemeRegistrationBuilder::new().scheme("kael")),
                )
            })
            .unwrap();

        assert_eq!(manifest.url_schemes(), &["kael".to_string()]);
        assert!(
            AppPackageManifestBuilder::new(AppMetadataBuilder::new("No Identifier"))
                .validate()
                .is_err()
        );
        assert!(
            cx.read(|app| {
                app.package_manifest_checked(
                    AppPackageManifestBuilder::new(
                        AppMetadataBuilder::new("Bad").identifier("com.example.bad"),
                    )
                    .url_schemes(UrlSchemeRegistrationBuilder::new().scheme("bad scheme")),
                )
            })
            .is_err()
        );
    }

    #[test]
    fn package_readiness_report_marks_ready_manifest() {
        let manifest = AppPackageManifestBuilder::new(
            AppMetadataBuilder::new("Kael Studio")
                .identifier("com.example.kael")
                .version("1.0.0"),
        )
        .icons(AppIconSetBuilder::new().icon(AppIconAssetBuilder::app("assets/app.icns")))
        .privacy_permissions(AppPrivacyManifestBuilder::new().permission(
            AppPrivacyPermissionBuilder::camera("Camera access records video notes."),
        ))
        .build_checked()
        .unwrap();

        let report = manifest.readiness_report();
        assert!(report.is_ready());
        assert!(report.issues().is_empty());
        assert_eq!(report.summary(), "package manifest is ready");
        assert_eq!(report.manifest().metadata().name(), "Kael Studio");
    }

    #[test]
    fn package_readiness_report_surfaces_errors_and_warnings() {
        let manifest = AppPackageManifestBuilder::new(
            AppMetadataBuilder::new("Kael Studio").identifier("com.example.kael"),
        )
        .file_associations(
            FileAssociationSetBuilder::new()
                .association(FileAssociationBuilder::new("Kael Project").extension("kaelproj")),
        )
        .privacy_permissions(AppPrivacyManifestBuilder::new().permission(
            AppPrivacyPermissionBuilder::screen_capture("Screen capture shares your workspace."),
        ))
        .build_checked()
        .unwrap();

        let report = AppPackageReadinessBuilder::new(manifest.clone()).evaluate();
        assert!(!report.is_ready());
        assert_eq!(report.errors().len(), 2);
        assert_eq!(report.warnings().len(), 3);
        assert!(
            report
                .issues()
                .iter()
                .any(|issue| issue.kind() == AppPackageReadinessIssueKind::MissingVersion)
        );
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AppPackageReadinessIssueKind::MissingAppIcon
                && issue.severity() == AppPackageReadinessSeverity::Error
        }));
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AppPackageReadinessIssueKind::PrivacyDeclarationWithoutUsageDescription
        }));
        assert_eq!(
            report.summary(),
            "package manifest has 2 error(s) and 3 warning(s)"
        );

        let relaxed = AppPackageReadinessBuilder::new(manifest)
            .allow_missing_version()
            .allow_missing_app_icon()
            .allow_missing_document_icon()
            .allow_extension_only_file_associations()
            .allow_privacy_declarations_without_platform_exports()
            .evaluate();
        assert!(relaxed.is_ready());
        assert!(relaxed.issues().is_empty());
    }

    #[test]
    fn package_readiness_checked_uses_builder() {
        let cx = TestAppContext::single();
        let manifest = AppPackageManifestBuilder::new(
            AppMetadataBuilder::new("Kael Studio")
                .identifier("com.example.kael")
                .version("1.0.0"),
        )
        .icons(AppIconSetBuilder::new().icon(AppIconAssetBuilder::app("assets/app.icns")))
        .build_checked()
        .unwrap();

        let report =
            cx.read(|app| app.package_readiness_checked(AppPackageReadinessBuilder::new(manifest)));
        assert!(report.is_ready());
    }

    #[test]
    fn distribution_plan_builder_validates_targets_and_artifacts() {
        let output_dir = std::env::temp_dir().join("kael-dist");
        let manifest = AppPackageManifestBuilder::new(
            AppMetadataBuilder::new("Kael Studio")
                .identifier("com.example.kael")
                .version("1.2.3"),
        )
        .icons(AppIconSetBuilder::new().icon(AppIconAssetBuilder::app("assets/app.icns")))
        .build_checked()
        .unwrap();

        let plan = AppDistributionPlanBuilder::new(&output_dir)
            .target(AppDistributionTargetBuilder::dmg())
            .target(AppDistributionTargetBuilder::msi().channel("stable"))
            .target(AppDistributionTargetBuilder::appimage())
            .build_checked()
            .unwrap();

        assert_eq!(plan.targets().len(), 3);
        assert_eq!(plan.targets()[0].platform(), AppDistributionPlatform::MacOs);
        assert_eq!(plan.targets()[1].format(), AppDistributionFormat::Msi);
        assert_eq!(plan.targets()[1].channel(), Some("stable"));
        assert_eq!(plan.targets_for(AppDistributionPlatform::Linux).len(), 1);
        assert_eq!(
            plan.artifact_paths(&manifest),
            vec![
                output_dir.join("kael-studio-1.2.3-macos.dmg"),
                output_dir.join("kael-studio-1.2.3-stable-windows.msi"),
                output_dir.join("kael-studio-1.2.3-linux.AppImage"),
            ]
        );

        assert!(
            AppDistributionPlanBuilder::new("relative/dist")
                .target(AppDistributionTargetBuilder::dmg())
                .validate()
                .is_err()
        );
        assert!(
            AppDistributionPlanBuilder::new(&output_dir)
                .validate()
                .is_err()
        );
        assert!(
            AppDistributionPlanBuilder::new(&output_dir)
                .target(AppDistributionTargetBuilder::dmg())
                .target(AppDistributionTargetBuilder::dmg())
                .validate()
                .is_err()
        );
        assert!(
            AppDistributionPlanBuilder::new(&output_dir)
                .target(AppDistributionTargetBuilder::msi().channel("bad channel"))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn distribution_plan_checked_uses_builder_validation() {
        let cx = TestAppContext::single();
        let output_dir = std::env::temp_dir().join("kael-dist");

        let plan = cx
            .read(|app| {
                app.distribution_plan_checked(
                    AppDistributionPlanBuilder::new(&output_dir)
                        .target(AppDistributionTargetBuilder::mac_zip())
                        .target(AppDistributionTargetBuilder::deb()),
                )
            })
            .unwrap();

        assert_eq!(plan.output_dir(), output_dir.as_path());
        assert_eq!(plan.targets_for(AppDistributionPlatform::MacOs).len(), 1);
        assert!(
            cx.read(|app| app.distribution_plan_checked(AppDistributionPlanBuilder::new("")))
                .is_err()
        );
    }

    #[test]
    fn signing_plan_builder_validates_platform_signing() {
        let dist = AppDistributionPlanBuilder::new(std::env::temp_dir().join("kael-dist-signing"))
            .target(AppDistributionTargetBuilder::dmg())
            .target(AppDistributionTargetBuilder::msi())
            .build_checked()
            .unwrap();

        let plan = AppSigningPlanBuilder::new()
            .target(
                AppSigningTargetBuilder::macos_developer_id(
                    "Developer ID Application: Example, Inc.",
                )
                .team_id("ABCDE12345")
                .hardened_runtime()
                .notarize(),
            )
            .target(AppSigningTargetBuilder::windows_authenticode(
                "Example Code Signing Cert",
            ))
            .build_checked()
            .unwrap();

        assert_eq!(plan.targets().len(), 2);
        assert!(plan.covers_distribution_plan(&dist));
        let mac = plan.target_for(AppDistributionPlatform::MacOs).unwrap();
        assert_eq!(
            mac.identity(),
            Some("Developer ID Application: Example, Inc.")
        );
        assert_eq!(mac.team_id(), Some("ABCDE12345"));
        assert!(mac.hardened_runtime());
        assert!(mac.notarize());
        assert!(mac.timestamp());

        assert!(AppSigningPlanBuilder::new().validate().is_err());
        assert!(
            AppSigningPlanBuilder::new()
                .target(AppSigningTargetBuilder::macos_developer_id(
                    "Developer ID Application: Example"
                ))
                .target(AppSigningTargetBuilder::macos_developer_id("Other"))
                .validate()
                .is_err()
        );
        assert!(
            AppSigningTargetBuilder::new(AppDistributionPlatform::Windows)
                .notarize()
                .validate()
                .is_err()
        );
        assert!(
            AppSigningTargetBuilder::new(AppDistributionPlatform::Linux)
                .hardened_runtime()
                .validate()
                .is_err()
        );
        assert!(
            AppSigningTargetBuilder::new(AppDistributionPlatform::MacOs)
                .notarize()
                .validate()
                .is_err()
        );
        assert!(
            AppSigningTargetBuilder::windows_authenticode(" bad")
                .validate()
                .is_err()
        );
        assert!(
            AppSigningTargetBuilder::macos_developer_id("Developer ID")
                .team_id("bad team")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn signing_plan_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let plan = cx
            .read(|app| {
                app.signing_plan_checked(AppSigningPlanBuilder::new().target(
                    AppSigningTargetBuilder::linux_package("kael-release-key").without_timestamp(),
                ))
            })
            .unwrap();

        let linux = plan.target_for(AppDistributionPlatform::Linux).unwrap();
        assert_eq!(linux.identity(), Some("kael-release-key"));
        assert!(!linux.timestamp());
        assert!(
            cx.read(|app| app.signing_plan_checked(AppSigningPlanBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn file_drop_intent_builder_validates_files_extensions_and_canonical_paths() {
        let root =
            std::env::temp_dir().join(format!("kael_file_drop_intent_{}", std::process::id()));
        let nested = root.join("nested");
        let file = nested.join("Clip.MP4");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&file, "video").unwrap();

        let relative_file = nested.join("..").join("nested").join("Clip.MP4");
        let intent = FileDropIntentBuilder::media_source()
            .path(relative_file)
            .path(file.clone())
            .max_paths(2)
            .canonicalize_paths()
            .build_checked()
            .unwrap();

        assert_eq!(intent.purpose().label(), "media-source");
        assert_eq!(intent.paths(), &[std::fs::canonicalize(&file).unwrap()]);
        assert_eq!(
            intent.first_path(),
            Some(std::fs::canonicalize(&file).unwrap().as_path())
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_drop_intent_builder_rejects_mismatched_drops() {
        let root =
            std::env::temp_dir().join(format!("kael_file_drop_reject_{}", std::process::id()));
        let file = root.join("notes.txt");
        let folder = root.join("folder");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&file, "notes").unwrap();

        assert!(
            FileDropIntentBuilder::new(FileDropPurpose::custom(" bad"))
                .path(&file)
                .validate()
                .is_err()
        );
        assert!(FileDropIntentBuilder::import_files().validate().is_err());
        assert!(
            FileDropIntentBuilder::import_files()
                .path(&folder)
                .validate()
                .is_err()
        );
        assert!(
            FileDropIntentBuilder::import_folder()
                .path(&file)
                .validate()
                .is_err()
        );
        assert!(
            FileDropIntentBuilder::open_document()
                .path(&file)
                .extensions(["md"])
                .validate()
                .is_err()
        );
        assert!(
            FileDropIntentBuilder::open_document()
                .path(&file)
                .max_paths(0)
                .validate()
                .is_err()
        );
        assert!(
            FileDropIntentBuilder::open_document()
                .path(root.join("missing.txt"))
                .validate()
                .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_drop_intent_checked_uses_builder_validation() {
        let cx = TestAppContext::single();
        let path = PathBuf::from("/virtual/report.md");

        let intent = cx
            .read(|app| {
                app.file_drop_intent_checked(
                    FileDropIntentBuilder::open_document()
                        .allow_missing_paths()
                        .path(path.clone())
                        .extensions(["md"]),
                )
            })
            .unwrap();

        assert_eq!(intent.paths(), &[path]);
        assert_eq!(intent.purpose(), &FileDropPurpose::OpenDocument);

        assert!(
            cx.read(|app| app.file_drop_intent_checked(FileDropIntentBuilder::open_document()))
                .is_err()
        );
        assert_eq!(
            FileDropIntentBuilder::project_workspace()
                .any_path_kind()
                .allow_missing_paths()
                .path("/virtual/workspace")
                .path_kind(),
            FileDropPathKind::Any
        );
    }

    #[test]
    fn file_export_drag_builder_validates_existing_and_virtual_items() {
        let root =
            std::env::temp_dir().join(format!("kael_file_export_drag_{}", std::process::id()));
        let file = root.join("report.pdf");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&file, "pdf").unwrap();

        let export = FileExportDragIntentBuilder::existing_files("Drag report to another app.")
            .path(&file)
            .virtual_file_with_mime("summary.txt", "text/plain", b"summary".to_vec())
            .max_items(2)
            .build_checked()
            .unwrap();

        assert_eq!(export.purpose(), "Drag report to another app.");
        assert_eq!(export.items().len(), 2);
        assert!(export.has_virtual_files());
        assert_eq!(
            export.required_capabilities(),
            vec![Capability::FilesystemRead {
                scope: PathScope::UserSelected
            }]
        );
        assert_eq!(export.items()[0].display_name(), "report.pdf");
        assert_eq!(export.items()[1].display_name(), "summary.txt");
        assert_eq!(export.items()[1].mime_type_hint(), Some("text/plain"));

        assert!(
            FileExportDragIntentBuilder::new("")
                .virtual_file("summary.txt", b"summary".to_vec())
                .validate()
                .is_err()
        );
        assert!(
            FileExportDragIntentBuilder::existing_files("Export")
                .path(root.join("missing.txt"))
                .validate()
                .is_err()
        );
        assert!(
            FileExportDragIntentBuilder::generated_files("Export")
                .virtual_file("../summary.txt", b"summary".to_vec())
                .validate()
                .is_err()
        );
        assert!(
            FileExportDragIntentBuilder::generated_files("Export")
                .virtual_file_with_mime("summary.txt", "text", b"summary".to_vec())
                .validate()
                .is_err()
        );
        assert!(
            FileExportDragIntentBuilder::generated_files("Export")
                .virtual_file("summary.txt", Vec::<u8>::new())
                .validate()
                .is_err()
        );
        assert!(
            FileExportDragIntentBuilder::generated_files("Export")
                .virtual_file("summary.txt", b"summary".to_vec())
                .max_virtual_file_bytes(1)
                .validate()
                .is_err()
        );
        assert!(
            FileExportDragIntentBuilder::generated_files("Export")
                .virtual_file("summary.txt", b"summary".to_vec())
                .max_items(0)
                .validate()
                .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_export_drag_checked_uses_builder_validation() {
        let cx = TestAppContext::single();
        let export = cx
            .read(|app| {
                app.file_export_drag_checked(
                    FileExportDragIntentBuilder::generated_files("Drag generated image.")
                        .virtual_file_with_mime("preview.png", "image/png", vec![1, 2, 3, 4]),
                )
            })
            .unwrap();

        assert_eq!(export.items().len(), 1);
        assert!(export.required_capabilities().is_empty());
        assert!(matches!(
            &export.items()[0],
            FileExportDragItem::VirtualFile { file_name, .. } if file_name == "preview.png"
        ));

        assert!(
            cx.read(|app| app.file_export_drag_checked(FileExportDragIntentBuilder::new("Export")))
                .is_err()
        );
    }

    #[test]
    fn file_intake_plan_builder_classifies_common_content() {
        let paths = [
            "/virtual/movie.MP4",
            "/virtual/song.flac",
            "/virtual/image.PNG",
            "/virtual/report.pdf",
            "/virtual/readme.md",
            "/virtual/data.json",
            "/virtual/archive.zip",
            "/virtual/app.code-workspace",
            "/virtual/unknown.bin",
        ];

        let plan = FileIntakePlanBuilder::new()
            .allow_missing_paths()
            .paths(paths)
            .build_checked()
            .unwrap();

        let kinds = plan
            .entries()
            .iter()
            .map(|entry| entry.kind())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                FileIntakeKind::Video,
                FileIntakeKind::Audio,
                FileIntakeKind::Image,
                FileIntakeKind::Pdf,
                FileIntakeKind::Text,
                FileIntakeKind::Data,
                FileIntakeKind::Archive,
                FileIntakeKind::Project,
                FileIntakeKind::Unknown,
            ]
        );
        assert!(plan.has_media());
        assert!(plan.has_unknown());
        assert_eq!(
            plan.paths_of_kind(FileIntakeKind::Video),
            vec![std::path::Path::new("/virtual/movie.MP4")]
        );
        assert_eq!(plan.entries()[0].extension(), Some("mp4"));
        assert!(FileIntakeKind::Video.is_media());
        assert!(FileIntakeKind::Text.is_document());
    }

    #[test]
    fn file_intake_plan_builder_validates_and_canonicalizes_paths() {
        let root =
            std::env::temp_dir().join(format!("kael_file_intake_plan_{}", std::process::id()));
        let nested = root.join("nested");
        let file = nested.join("notes.md");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&file, "notes").unwrap();

        let relative_file = nested.join("..").join("nested").join("notes.md");
        let plan = FileIntakePlanBuilder::new()
            .canonicalize_paths()
            .path(relative_file)
            .path(file.clone())
            .build_checked()
            .unwrap();

        assert_eq!(plan.entries().len(), 1);
        assert_eq!(plan.entries()[0].kind(), FileIntakeKind::Text);
        assert_eq!(
            plan.entries()[0].path(),
            std::fs::canonicalize(&file).unwrap()
        );

        assert!(
            FileIntakePlanBuilder::new()
                .allow_missing_paths()
                .path("/virtual/unknown.bin")
                .reject_unknown()
                .validate()
                .is_err()
        );
        assert!(FileIntakePlanBuilder::new().validate().is_err());
        assert!(
            FileIntakePlanBuilder::new()
                .allow_missing_paths()
                .path("/virtual/a.md")
                .max_paths(0)
                .validate()
                .is_err()
        );
        assert!(
            FileIntakePlanBuilder::new()
                .path(root.join("missing.md"))
                .validate()
                .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_intake_plan_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let plan = cx
            .read(|app| {
                app.file_intake_plan_checked(
                    FileIntakePlanBuilder::new()
                        .allow_missing_paths()
                        .path("/virtual/project.kaelproj"),
                )
            })
            .unwrap();

        assert_eq!(plan.entries()[0].kind(), FileIntakeKind::Project);
        assert!(
            cx.read(|app| app.file_intake_plan_checked(FileIntakePlanBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn recent_documents_builder_validates_and_dedupes() {
        let first = PathBuf::from("/tmp/first.txt");
        let second = PathBuf::from("/tmp/second.txt");
        let documents = RecentDocumentsBuilder::new()
            .document(first.clone())
            .document(first.clone())
            .documents([second.clone()]);

        assert_eq!(documents.configured_documents(), &[first, second]);
        assert!(!documents.requires_existing_files());
        assert!(!documents.canonicalizes_paths());
        assert!(documents.validate().is_ok());
        assert!(RecentDocumentsBuilder::new().validate().is_err());
    }

    #[test]
    fn recent_documents_builder_can_require_existing_canonical_files() {
        let root = std::env::temp_dir().join(format!(
            "kael_recent_documents_builder_{}",
            std::process::id()
        ));
        let nested = root.join("nested");
        let file = nested.join("report.md");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&file, "report").unwrap();

        let relative_file = nested.join("..").join("nested").join("report.md");
        let documents = RecentDocumentsBuilder::new()
            .require_existing_files()
            .canonicalize()
            .document(relative_file)
            .document(file.clone())
            .build()
            .unwrap();

        assert_eq!(documents, vec![std::fs::canonicalize(&file).unwrap()]);
        assert!(
            RecentDocumentsBuilder::new()
                .require_existing_files()
                .document(root.join("missing.md"))
                .validate()
                .is_err()
        );
        assert!(
            RecentDocumentsBuilder::new()
                .require_existing_files()
                .document(&nested)
                .validate()
                .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn add_recent_documents_uses_builder_options() {
        let cx = TestAppContext::single();
        let first = PathBuf::from("/tmp/first.txt");
        let second = PathBuf::from("/tmp/second.txt");

        let added = cx
            .read(|app| {
                app.add_recent_documents(
                    RecentDocumentsBuilder::new()
                        .document(first.clone())
                        .document(first.clone())
                        .document(second.clone()),
                )
            })
            .unwrap();

        assert_eq!(added, vec![first.clone(), second.clone()]);
        assert_eq!(cx.recent_documents(), vec![first, second]);
    }

    #[test]
    fn jump_list_builder_validates_tasks_and_workspaces() {
        let first = PathBuf::from("/tmp/project-a");
        let second = PathBuf::from("/tmp/project-b");
        let jump_list = JumpListBuilder::new()
            .action("Open Project", JumpListOpen)
            .workspace([first.clone(), second.clone()])
            .workspace([first.clone(), second.clone()]);

        assert_eq!(jump_list.menus().len(), 1);
        assert_eq!(jump_list.entries().len(), 1);
        assert!(!jump_list.requires_existing_paths());
        assert!(!jump_list.canonicalizes_paths());
        assert!(jump_list.validate().is_ok());

        let (_menus, entries) = jump_list.build_checked().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].as_slice(), &[first, second]);

        assert!(JumpListBuilder::new().validate().is_err());
        assert!(
            JumpListBuilder::new()
                .workspace(Vec::<PathBuf>::new())
                .validate()
                .is_err()
        );
        assert!(
            JumpListBuilder::new()
                .workspace_path(PathBuf::new())
                .validate()
                .is_err()
        );
        assert!(
            JumpListBuilder::new()
                .action(" Open Project", JumpListOpen)
                .validate()
                .is_err()
        );
        assert!(
            JumpListBuilder::new()
                .menu_item(MenuItem::separator())
                .validate()
                .is_err()
        );
    }

    #[test]
    fn jump_list_builder_can_require_existing_canonical_paths() {
        let root =
            std::env::temp_dir().join(format!("kael_jump_list_builder_{}", std::process::id()));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let relative_workspace = workspace.join("..").join("workspace");
        let (_menus, entries) = JumpListBuilder::new()
            .action("Open Project", JumpListOpen)
            .require_existing_paths()
            .canonicalize()
            .workspace_path(relative_workspace)
            .build_checked()
            .unwrap();

        assert_eq!(
            entries[0].as_slice(),
            &[std::fs::canonicalize(&workspace).unwrap()]
        );
        assert!(
            JumpListBuilder::new()
                .require_existing_paths()
                .workspace_path(root.join("missing"))
                .validate()
                .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_jump_list_checked_validates_before_platform_call() {
        let cx = TestAppContext::single();

        assert!(
            cx.read(|app| {
                app.update_jump_list_checked(
                    JumpListBuilder::new()
                        .action("Open Project", JumpListOpen)
                        .workspace_path("/tmp/project"),
                )
            })
            .is_ok()
        );
        assert!(
            cx.read(|app| {
                app.update_jump_list_checked(
                    JumpListBuilder::new().menu_item(MenuItem::separator()),
                )
            })
            .is_err()
        );
    }

    #[test]
    fn permission_request_builder_validates_configured_permissions() {
        assert!(PermissionRequestBuilder::new().validate().is_err());

        let permissions = PermissionRequestBuilder::new()
            .accessibility()
            .media_devices();

        assert!(permissions.validate().is_ok());
        assert!(permissions.requests_accessibility());
        assert!(permissions.requests_microphone());
        assert!(permissions.requests_camera());

        let startup = PermissionRequestBuilder::startup_privacy();
        assert!(startup.requests_accessibility());
        assert!(startup.requests_microphone());
        assert!(startup.requests_camera());

        let capture = PermissionRequestBuilder::capture_studio();
        assert!(capture.requests_accessibility());
        assert!(capture.requests_microphone());
        assert!(capture.requests_camera());
    }

    #[test]
    fn request_permissions_returns_status_snapshot() {
        let cx = TestAppContext::single();

        let result = cx
            .read(|app| {
                app.request_permissions(
                    PermissionRequestBuilder::new()
                        .accessibility()
                        .microphone()
                        .camera(),
                )
            })
            .unwrap();

        assert_eq!(result.accessibility, Some(PermissionStatus::Granted));
        assert_eq!(result.microphone, Some(PermissionStatus::Granted));
        assert_eq!(result.camera, Some(PermissionStatus::Granted));
        assert!(!result.prompted());
        assert!(!result.has_blocking_denial());
        assert!(result.all_granted());
        assert!(result.blocking_denials().is_empty());
        assert_eq!(result.blocking_denial_summary(), None);
        assert!(!result.has_pending_permission());
        assert!(result.pending_permissions().is_empty());
        assert_eq!(result.requested_permissions().len(), 3);
        assert_eq!(result.granted_permissions().len(), 3);
        assert_eq!(
            result.granted_summary().as_deref(),
            Some("Accessibility, Microphone, Camera")
        );
    }

    #[test]
    fn permission_request_result_reports_blocking_denials() {
        let result = super::PermissionRequestResult {
            accessibility: Some(PermissionStatus::Denied),
            microphone: Some(PermissionStatus::Granted),
            camera: Some(PermissionStatus::Restricted),
            requested_accessibility: false,
            requested_microphone: false,
            requested_camera: false,
        };

        assert!(!result.all_granted());
        assert!(result.has_blocking_denial());
        let denials = result.blocking_denials();
        assert_eq!(denials.len(), 2);
        assert_eq!(denials[0].key, "accessibility");
        assert_eq!(denials[0].label, "Accessibility");
        assert_eq!(denials[0].status, PermissionStatus::Denied);
        assert!(!denials[0].is_restricted());
        assert_eq!(denials[1].key, "camera");
        assert_eq!(denials[1].status, PermissionStatus::Restricted);
        assert!(denials[1].is_restricted());
        assert_eq!(
            result.blocking_denial_summary().as_deref(),
            Some("Accessibility: Denied, Camera: Restricted")
        );
    }

    #[test]
    fn permission_request_result_reports_pending_permissions() {
        let result = super::PermissionRequestResult {
            accessibility: Some(PermissionStatus::NotDetermined),
            microphone: Some(PermissionStatus::Denied),
            camera: Some(PermissionStatus::Granted),
            requested_accessibility: true,
            requested_microphone: true,
            requested_camera: false,
        };

        assert!(result.has_pending_permission());
        let pending = result.pending_permissions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].key, "accessibility");
        assert_eq!(pending[0].status, PermissionStatus::NotDetermined);
        assert_eq!(result.granted_permissions().len(), 1);
        assert_eq!(result.granted_summary().as_deref(), Some("Camera"));
    }

    #[test]
    fn auto_launch_builder_validates_app_id() {
        assert!(
            AutoLaunchBuilder::enable("com.example.app")
                .validate()
                .is_ok()
        );
        assert!(AutoLaunchBuilder::enable(" ").validate().is_err());
        assert!(
            AutoLaunchBuilder::enable(" com.example.app")
                .validate()
                .is_err()
        );
        assert!(
            AutoLaunchBuilder::enable("com.example.app ")
                .validate()
                .is_err()
        );
        assert!(
            AutoLaunchBuilder::enable("com.example app")
                .validate()
                .is_err()
        );
        assert!(
            AutoLaunchBuilder::enable("com.example\napp")
                .validate()
                .is_err()
        );

        let disabled = AutoLaunchBuilder::enable("com.example.app").enabled(false);
        assert_eq!(disabled.app_id(), "com.example.app");
        assert!(!disabled.requested_enabled());

        assert_eq!(
            disabled.build_checked().unwrap(),
            ("com.example.app".to_string(), false)
        );
    }

    #[test]
    fn configure_auto_launch_returns_platform_state() {
        let cx = TestAppContext::single();

        let enabled = cx
            .read(|app| app.configure_auto_launch(AutoLaunchBuilder::enable("com.example.app")))
            .unwrap();
        assert_eq!(enabled.app_id(), "com.example.app");
        assert!(enabled.enabled());
        assert!(cx.read(|app| app.is_auto_launch_enabled("com.example.app")));

        let disabled = cx
            .read(|app| app.configure_auto_launch(AutoLaunchBuilder::disable("com.example.app")))
            .unwrap();
        assert_eq!(disabled.app_id(), "com.example.app");
        assert!(!disabled.enabled());
        assert!(!cx.read(|app| app.is_auto_launch_enabled("com.example.app")));
    }

    #[test]
    fn app_path_builder_validates_roles_and_app_id() {
        let builder = AppPathBuilder::new("com.example.app").app_storage();
        assert!(builder.validate().is_ok());
        assert_eq!(builder.app_id(), "com.example.app");
        assert_eq!(
            builder.configured_roles(),
            &[
                AppPathRole::Data,
                AppPathRole::Config,
                AppPathRole::Cache,
                AppPathRole::Logs,
                AppPathRole::Temp,
            ]
        );
        assert!(!builder.creates_dirs());

        assert!(
            AppPathBuilder::new("")
                .role(AppPathRole::Data)
                .validate()
                .is_err()
        );
        assert!(
            AppPathBuilder::new(" com.example.app")
                .role(AppPathRole::Data)
                .validate()
                .is_err()
        );
        assert!(
            AppPathBuilder::new("com.example app")
                .role(AppPathRole::Data)
                .validate()
                .is_err()
        );
        assert!(
            AppPathBuilder::new("com/example/app")
                .role(AppPathRole::Data)
                .validate()
                .is_err()
        );
        assert!(AppPathBuilder::new("com.example.app").validate().is_err());
        assert!(
            AppPathBuilder::new("com.example.app")
                .role(AppPathRole::Data)
                .role(AppPathRole::Data)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_path_builder_resolves_common_paths() {
        let set = AppPathBuilder::new("com.example.app")
            .roles([
                AppPathRole::Data,
                AppPathRole::Config,
                AppPathRole::Cache,
                AppPathRole::Logs,
                AppPathRole::Temp,
                AppPathRole::Downloads,
            ])
            .build_checked()
            .unwrap();

        assert_eq!(set.app_id(), "com.example.app");
        assert_eq!(set.paths().len(), 6);
        assert!(set.data_dir().unwrap().ends_with("com.example.app"));
        assert!(set.config_dir().unwrap().ends_with("com.example.app"));
        assert!(set.cache_dir().unwrap().ends_with("com.example.app"));
        assert!(
            set.logs_dir()
                .unwrap()
                .to_string_lossy()
                .contains("com.example.app")
        );
        assert!(set.temp_dir().unwrap().ends_with("com.example.app"));
        assert!(set.downloads_dir().unwrap().ends_with("Downloads"));
        assert_eq!(set.get(AppPathRole::Cache), Some(set.cache_dir().unwrap()));
    }

    #[test]
    fn app_path_builder_can_create_temp_dir() {
        let app_id = format!("kael-app-path-test-{}", std::process::id());
        let set = AppPathBuilder::new(&app_id)
            .role(AppPathRole::Temp)
            .create_dirs()
            .build_checked()
            .unwrap();
        let temp_dir = set.temp_dir().unwrap().to_path_buf();

        assert!(temp_dir.is_dir());

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn app_paths_checked_resolves_paths_from_context() {
        let cx = TestAppContext::single();

        let set = cx
            .read(|app| {
                app.app_paths_checked(
                    AppPathBuilder::new("com.example.app").role(AppPathRole::Temp),
                )
            })
            .unwrap();

        assert!(set.temp_dir().unwrap().ends_with("com.example.app"));
    }

    #[test]
    fn app_storage_plan_builder_validates_entries_and_resolves_paths() {
        let plan = AppStoragePlanBuilder::new("com.example.app")
            .settings_json("settings", "settings.json")
            .sqlite_database("main-db", "state/app.sqlite")
            .key_value_store("kv", "kv")
            .blob_cache("thumbs", "thumbnails")
            .log_file("main-log", "app.log")
            .temp_workspace("exports", "exports")
            .build_checked()
            .unwrap();

        assert_eq!(plan.app_id(), "com.example.app");
        assert_eq!(plan.entries().len(), 6);
        assert!(plan.paths().data_dir().is_some());
        assert!(plan.paths().config_dir().is_some());
        assert!(plan.paths().cache_dir().is_some());
        assert!(plan.paths().logs_dir().is_some());
        assert!(plan.paths().temp_dir().is_some());

        let settings = plan.entry("settings").unwrap();
        assert_eq!(settings.kind(), &AppStorageKind::SettingsJson);
        assert_eq!(settings.durability(), AppStorageDurability::Durable);
        assert_eq!(settings.role(), AppPathRole::Config);
        assert_eq!(settings.relative_path(), Path::new("settings.json"));
        assert!(settings.absolute_path().ends_with("settings.json"));
        assert_eq!(
            settings.write_capability(),
            Capability::FilesystemWrite {
                scope: PathScope::AppData
            }
        );
        assert_eq!(
            settings.read_capability(),
            Capability::FilesystemRead {
                scope: PathScope::AppData
            }
        );

        assert_eq!(
            plan.entries_with_durability(AppStorageDurability::Rebuildable)
                .len(),
            2
        );
        assert!(plan.declared_max_bytes() > 0);
    }

    #[test]
    fn app_storage_plan_builder_rejects_unsafe_storage_contracts() {
        assert!(
            AppStoragePlanBuilder::new("")
                .settings_json("settings", "settings.json")
                .validate()
                .is_err()
        );
        assert!(
            AppStoragePlanBuilder::new("com.example.app")
                .validate()
                .is_err()
        );
        assert!(
            AppStoragePlanBuilder::new("com.example.app")
                .settings_json("settings", "settings.json")
                .settings_json("settings", "other.json")
                .validate()
                .is_err()
        );
        assert!(
            AppStoragePlanBuilder::new("com.example.app")
                .entry(AppStorageEntryBuilder::settings_json(
                    "bad/id",
                    "settings.json"
                ))
                .validate()
                .is_err()
        );
        assert!(
            AppStoragePlanBuilder::new("com.example.app")
                .entry(AppStorageEntryBuilder::settings_json(
                    "settings",
                    "../settings.json"
                ))
                .validate()
                .is_err()
        );
        assert!(
            AppStoragePlanBuilder::new("com.example.app")
                .entry(AppStorageEntryBuilder::new(
                    "downloads",
                    AppStorageKind::Custom("download-export".into()),
                    AppPathRole::Downloads,
                    "file.bin",
                ))
                .validate()
                .is_err()
        );
        assert!(
            AppStoragePlanBuilder::new("com.example.app")
                .entry(
                    AppStorageEntryBuilder::settings_json("settings", "settings.json").max_bytes(0)
                )
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_storage_plan_checked_uses_builder_validation() {
        let cx = TestAppContext::single();
        let plan = cx
            .read(|app| {
                app.app_storage_plan_checked(
                    AppStoragePlanBuilder::new("com.example.kael")
                        .settings_json("settings", "settings.json")
                        .blob_cache("assets", "assets")
                        .entry(
                            AppStorageEntryBuilder::key_value_store("tokens", "tokens").sensitive(),
                        ),
                )
            })
            .unwrap();

        assert_eq!(plan.entries().len(), 3);
        assert!(plan.entry("tokens").unwrap().is_sensitive());
        assert_eq!(
            plan.entry("assets").unwrap().durability(),
            AppStorageDurability::Rebuildable
        );

        assert!(
            cx.read(|app| app.app_storage_plan_checked(AppStoragePlanBuilder::new("com.example")))
                .is_err()
        );
    }

    #[test]
    fn app_metadata_builder_validates_identity_fields() {
        let metadata = AppMetadataBuilder::new("Kael Studio")
            .version("1.2.3")
            .build("2026.07.01")
            .identifier("com.example.kael")
            .website_url("https://example.com")
            .support_url("https://example.com/support")
            .copyright("Copyright 2026 Example")
            .license("Apache-2.0")
            .credits("Built with Kael")
            .build_checked()
            .unwrap();

        assert_eq!(metadata.name(), "Kael Studio");
        assert_eq!(metadata.version(), Some("1.2.3"));
        assert_eq!(metadata.build(), Some("2026.07.01"));
        assert_eq!(metadata.identifier(), Some("com.example.kael"));
        assert_eq!(metadata.website_url(), Some("https://example.com"));
        assert_eq!(metadata.support_url(), Some("https://example.com/support"));
        assert_eq!(metadata.display_title(), "Kael Studio 1.2.3");

        assert!(AppMetadataBuilder::new("").validate().is_err());
        assert!(AppMetadataBuilder::new(" Kael").validate().is_err());
        assert!(
            AppMetadataBuilder::new("Kael")
                .version("1.0\nbad")
                .validate()
                .is_err()
        );
        assert!(
            AppMetadataBuilder::new("Kael")
                .identifier("com/example/kael")
                .validate()
                .is_err()
        );
        assert!(
            AppMetadataBuilder::new("Kael")
                .website_url("file:///tmp/app")
                .validate()
                .is_err()
        );
        assert!(
            AppMetadataBuilder::new("Kael")
                .support_url("https://")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_metadata_builds_about_dialog() {
        let metadata = AppMetadataBuilder::new("Kael Studio")
            .version("1.2.3")
            .build("abc123")
            .identifier("com.example.kael")
            .support_url("https://example.com/support")
            .license("Apache-2.0")
            .build_checked()
            .unwrap();

        let dialog = metadata.about_dialog();

        assert_eq!(dialog.title().as_ref(), "About Kael Studio");
        assert_eq!(dialog.message().as_ref(), "Kael Studio 1.2.3");
        let detail = dialog.detail_text().unwrap().as_ref();
        assert!(detail.contains("Build: abc123"));
        assert!(detail.contains("Identifier: com.example.kael"));
        assert!(detail.contains("Support: https://example.com/support"));
        assert!(detail.contains("License: Apache-2.0"));
        assert!(dialog.validate().is_ok());
    }

    #[test]
    fn show_about_dialog_checked_validates_metadata() {
        let cx = TestAppContext::single();

        assert!(
            cx.read(|app| app.show_about_dialog_checked(AppMetadataBuilder::new("Kael")))
                .is_ok()
        );
        assert!(
            cx.read(|app| app.show_about_dialog_checked(AppMetadataBuilder::new(" Kael")))
                .is_err()
        );
    }

    #[test]
    fn app_update_release_builder_validates_metadata() {
        let release = AppUpdateReleaseBuilder::new("1.2.3")
            .channel(AppUpdateChannel::Stable)
            .title("Kael 1.2.3")
            .notes("Bug fixes and performance improvements.")
            .notes_url("https://example.com/releases/1.2.3")
            .download_url("https://example.com/downloads/kael-1.2.3.dmg")
            .critical()
            .mandatory()
            .signed()
            .rollout_percentage(25)
            .build_checked()
            .unwrap();

        assert_eq!(release.version(), "1.2.3");
        assert_eq!(release.channel().unwrap().label(), "stable");
        assert_eq!(release.title(), Some("Kael 1.2.3"));
        assert_eq!(
            release.notes(),
            Some("Bug fixes and performance improvements.")
        );
        assert_eq!(
            release.notes_url(),
            Some("https://example.com/releases/1.2.3")
        );
        assert_eq!(
            release.download_url(),
            Some("https://example.com/downloads/kael-1.2.3.dmg")
        );
        assert!(release.is_critical());
        assert!(release.is_mandatory());
        assert!(release.is_signed());
        assert_eq!(release.rollout_percentage(), Some(25));
        assert_eq!(release.display_title(), "Kael 1.2.3");

        assert!(AppUpdateReleaseBuilder::new("").validate().is_err());
        assert!(
            AppUpdateReleaseBuilder::new("1.2.3")
                .title(" bad")
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateReleaseBuilder::new("1.2.3")
                .download_url("file:///tmp/update.zip")
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateReleaseBuilder::new("1.2.3")
                .channel(AppUpdateChannel::custom(" beta"))
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateReleaseBuilder::new("1.2.3")
                .rollout_percentage(101)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_update_offer_policy_evaluates_release_eligibility() {
        let policy = AppUpdateOfferPolicyBuilder::stable()
            .rollout_bucket(10)
            .build_checked()
            .unwrap();
        let release = AppUpdateReleaseBuilder::new("1.2.3")
            .channel(AppUpdateChannel::Stable)
            .download_url("https://example.com/downloads/kael-1.2.3.dmg")
            .signed()
            .rollout_percentage(25)
            .build_checked()
            .unwrap();

        let decision = policy.evaluate_release(&release);
        assert_eq!(decision.kind(), AppUpdateOfferKind::Offer);
        assert_eq!(decision.reason(), AppUpdateOfferReason::Eligible);
        assert!(decision.should_offer());

        let unsigned = AppUpdateReleaseBuilder::new("1.2.4")
            .download_url("https://example.com/downloads/kael-1.2.4.dmg")
            .build_checked()
            .unwrap();
        let decision = policy.evaluate_release(&unsigned);
        assert_eq!(decision.kind(), AppUpdateOfferKind::Block);
        assert_eq!(decision.reason(), AppUpdateOfferReason::UnsignedRelease);
        assert!(decision.is_blocked());

        let notes_only = AppUpdateReleaseBuilder::new("1.2.5")
            .notes_url("https://example.com/releases/1.2.5")
            .signed()
            .build_checked()
            .unwrap();
        assert_eq!(
            policy.evaluate_release(&notes_only).reason(),
            AppUpdateOfferReason::MissingDownloadUrl
        );

        let allowed_notes = AppUpdateOfferPolicyBuilder::stable()
            .allow_release_notes_only()
            .build_checked()
            .unwrap();
        assert!(allowed_notes.evaluate_release(&notes_only).should_offer());

        let beta = AppUpdateReleaseBuilder::new("1.3.0")
            .channel(AppUpdateChannel::Beta)
            .download_url("https://example.com/downloads/kael-1.3.0.dmg")
            .signed()
            .build_checked()
            .unwrap();
        let decision = policy.evaluate_release(&beta);
        assert_eq!(decision.kind(), AppUpdateOfferKind::Defer);
        assert_eq!(decision.reason(), AppUpdateOfferReason::ChannelMismatch);

        let rollout_excluded = AppUpdateReleaseBuilder::new("1.4.0")
            .download_url("https://example.com/downloads/kael-1.4.0.dmg")
            .signed()
            .rollout_percentage(25)
            .build_checked()
            .unwrap();
        let excluded_policy = AppUpdateOfferPolicyBuilder::stable()
            .rollout_bucket(90)
            .build_checked()
            .unwrap();
        assert_eq!(
            excluded_policy.evaluate_release(&rollout_excluded).reason(),
            AppUpdateOfferReason::RolloutExcluded
        );

        let critical = AppUpdateReleaseBuilder::new("1.4.1")
            .download_url("https://example.com/downloads/kael-1.4.1.dmg")
            .signed()
            .critical()
            .rollout_percentage(1)
            .build_checked()
            .unwrap();
        assert!(excluded_policy.evaluate_release(&critical).should_offer());

        assert!(
            AppUpdateOfferPolicyBuilder::stable()
                .rollout_bucket(100)
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateOfferPolicyBuilder::stable()
                .cohort_key(" user-123")
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateOfferPolicyBuilder::stable()
                .rollout_bucket(1)
                .cohort_key("user-123")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_update_offer_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let decision = cx
            .read(|app| {
                app.app_update_offer_checked(
                    AppUpdateOfferPolicyBuilder::stable(),
                    AppUpdateReleaseBuilder::new("1.2.3")
                        .download_url("https://example.com/downloads/kael-1.2.3.dmg")
                        .signed()
                        .mandatory(),
                )
            })
            .unwrap();

        assert!(decision.should_offer());
        assert!(decision.is_mandatory());
        assert!(
            cx.read(|app| {
                app.app_update_offer_checked(
                    AppUpdateOfferPolicyBuilder::stable().rollout_bucket(100),
                    AppUpdateReleaseBuilder::new("1.2.3"),
                )
            })
            .is_err()
        );
    }

    #[test]
    fn app_update_state_builder_drives_checked_ui_actions() {
        let state = AppUpdateStateBuilder::new("1.0.0")
            .channel(AppUpdateChannel::Beta)
            .phase(AppUpdatePhase::Available)
            .release(
                AppUpdateReleaseBuilder::new("1.1.0")
                    .title("Kael 1.1")
                    .download_url("https://example.com/kael-1.1.zip"),
            )
            .build_checked()
            .unwrap();

        assert_eq!(state.current_version(), "1.0.0");
        assert_eq!(state.channel().label(), "beta");
        assert_eq!(state.phase(), AppUpdatePhase::Available);
        assert!(state.has_update());
        assert_eq!(state.recommended_action(), AppUpdateAction::Download);
        assert_eq!(state.menu_label(), "Download Kael 1.1");

        let release_notes_only = AppUpdateStateBuilder::new("1.0.0")
            .phase(AppUpdatePhase::Available)
            .release(
                AppUpdateReleaseBuilder::new("1.1.0")
                    .notes_url("https://example.com/releases/1.1.0"),
            )
            .build_checked()
            .unwrap();
        assert_eq!(
            release_notes_only.recommended_action(),
            AppUpdateAction::OpenReleaseNotes
        );

        let downloading = AppUpdateStateBuilder::new("1.0.0")
            .phase(AppUpdatePhase::Downloading)
            .release(AppUpdateReleaseBuilder::new("1.1.0"))
            .download_progress(0.42)
            .build_checked()
            .unwrap();
        assert_eq!(downloading.recommended_action(), AppUpdateAction::Wait);
        assert_eq!(downloading.download_progress(), Some(0.42));
        assert_eq!(downloading.menu_label(), "Downloading Update 42%");

        let failed = AppUpdateStateBuilder::new("1.0.0")
            .phase(AppUpdatePhase::Failed)
            .error_message("network unavailable")
            .build_checked()
            .unwrap();
        assert_eq!(failed.recommended_action(), AppUpdateAction::Retry);
        assert_eq!(failed.error_message(), Some("network unavailable"));
    }

    #[test]
    fn app_update_state_builder_rejects_inconsistent_states() {
        assert!(
            AppUpdateStateBuilder::new("")
                .phase(AppUpdatePhase::Idle)
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateStateBuilder::new("1.0.0")
                .channel(AppUpdateChannel::custom(" beta"))
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateStateBuilder::new("1.0.0")
                .phase(AppUpdatePhase::Available)
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateStateBuilder::new("1.0.0")
                .phase(AppUpdatePhase::Checking)
                .download_progress(0.5)
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateStateBuilder::new("1.0.0")
                .phase(AppUpdatePhase::Downloading)
                .release(AppUpdateReleaseBuilder::new("1.1.0"))
                .download_progress(1.5)
                .validate()
                .is_err()
        );
        assert!(
            AppUpdateStateBuilder::new("1.0.0")
                .phase(AppUpdatePhase::Failed)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_update_state_checked_uses_builder_validation() {
        let cx = TestAppContext::single();

        let state = cx
            .read(|app| {
                app.app_update_state_checked(
                    AppUpdateStateBuilder::new("1.0.0").phase(AppUpdatePhase::UpToDate),
                )
            })
            .unwrap();
        assert_eq!(state.menu_label(), "Up to Date");

        assert!(
            cx.read(|app| app.app_update_state_checked(AppUpdateStateBuilder::new("")))
                .is_err()
        );
    }

    #[test]
    fn launch_context_builder_validates_environment_allowlist() {
        let builder = LaunchContextBuilder::new()
            .without_args()
            .environment_keys(["PATH", "HOME"])
            .require_executable()
            .require_current_dir();
        assert!(builder.validate().is_ok());
        assert!(!builder.captures_args());
        assert_eq!(
            builder.environment_allowlist(),
            &["PATH".to_string(), "HOME".to_string()]
        );

        assert!(
            LaunchContextBuilder::new()
                .environment_key("")
                .validate()
                .is_err()
        );
        assert!(
            LaunchContextBuilder::new()
                .environment_key(" PATH")
                .validate()
                .is_err()
        );
        assert!(
            LaunchContextBuilder::new()
                .environment_key("BAD=VALUE")
                .validate()
                .is_err()
        );
        assert!(
            LaunchContextBuilder::new()
                .environment_key("PATH")
                .environment_key("PATH")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn launch_context_builder_captures_checked_snapshot() {
        let snapshot = LaunchContextBuilder::new()
            .environment_key("PATH")
            .require_executable()
            .require_current_dir()
            .capture_checked()
            .unwrap();

        assert_eq!(snapshot.process_id(), std::process::id());
        assert!(snapshot.executable_path().is_some());
        assert!(snapshot.current_dir().is_some());
        assert!(!snapshot.args().is_empty());
        assert_eq!(snapshot.is_debug_build(), cfg!(debug_assertions));
        assert_eq!(snapshot.is_development_mode(), cfg!(debug_assertions));
        if std::env::var("PATH").is_ok() {
            assert!(snapshot.env("PATH").is_some());
        }

        let no_args = LaunchContextBuilder::new()
            .without_args()
            .capture_checked()
            .unwrap();
        assert!(no_args.args().is_empty());
        assert!(no_args.environment().is_empty());
    }

    #[test]
    fn app_launch_context_uses_checked_builder() {
        let cx = TestAppContext::single();

        let snapshot = cx.read(|app| app.launch_context());
        assert_eq!(snapshot.process_id(), std::process::id());
        assert!(snapshot.environment().is_empty());

        let checked = cx
            .read(|app| {
                app.launch_context_checked(
                    LaunchContextBuilder::new()
                        .without_args()
                        .environment_key("PATH")
                        .require_current_dir(),
                )
            })
            .unwrap();
        assert!(checked.args().is_empty());
        assert!(checked.current_dir().is_some());

        assert!(
            cx.read(|app| app
                .launch_context_checked(LaunchContextBuilder::new().environment_key("BAD=VALUE")))
                .is_err()
        );
    }

    #[test]
    fn locale_snapshot_builder_normalizes_locale_candidates() {
        let snapshot = LocaleSnapshotBuilder::new()
            .use_system_environment(false)
            .locale_from("de_DE.UTF-8", "test")
            .preferred_languages(["fr_FR", "de-DE", "fr-FR"])
            .build_checked()
            .unwrap();

        assert_eq!(snapshot.locale(), "de-DE");
        assert_eq!(snapshot.language(), "de");
        assert_eq!(snapshot.region(), Some("DE"));
        assert_eq!(snapshot.source(), Some("test"));
        assert_eq!(snapshot.text_direction(), LocaleTextDirection::LeftToRight);
        assert!(!snapshot.is_rtl());
        assert_eq!(
            snapshot.preferred_languages(),
            &["fr-FR".to_string(), "de-DE".to_string()]
        );
    }

    #[test]
    fn locale_snapshot_builder_handles_rtl_and_fallback() {
        let rtl = LocaleSnapshotBuilder::new()
            .use_system_environment(false)
            .locale("ar_SA")
            .build_checked()
            .unwrap();
        assert_eq!(rtl.locale(), "ar-SA");
        assert_eq!(rtl.text_direction(), LocaleTextDirection::RightToLeft);
        assert!(rtl.is_rtl());

        let fallback = LocaleSnapshotBuilder::new()
            .use_system_environment(false)
            .locale("C")
            .fallback_locale("ja_JP")
            .build_checked()
            .unwrap();
        assert_eq!(fallback.locale(), "ja-JP");
        assert_eq!(fallback.source(), Some("fallback"));

        assert!(
            LocaleSnapshotBuilder::new()
                .use_system_environment(false)
                .locale("not valid")
                .validate()
                .is_err()
        );
        assert!(
            LocaleSnapshotBuilder::new()
                .use_system_environment(false)
                .locale("en-US")
                .fallback_locale("bad locale")
                .validate()
                .is_err()
        );
        assert!(
            LocaleSnapshotBuilder::new()
                .use_system_environment(false)
                .locale("C")
                .require_locale()
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn app_locale_snapshot_uses_checked_builder() {
        let cx = TestAppContext::single();

        let snapshot = cx.read(|app| app.locale_snapshot());
        assert!(!snapshot.locale().is_empty());
        assert!(!snapshot.preferred_languages().is_empty());

        let checked = cx
            .read(|app| {
                app.locale_snapshot_checked(
                    LocaleSnapshotBuilder::new()
                        .use_system_environment(false)
                        .locale("he_IL"),
                )
            })
            .unwrap();
        assert_eq!(checked.locale(), "he-IL");
        assert!(checked.is_rtl());
    }

    #[test]
    fn text_checking_request_builder_validates_policy() {
        let request = TextCheckingRequest::builder("helo world")
            .locale("en_US")
            .check_grammar()
            .autocorrect()
            .custom_words(["Kael", "GPUI"])
            .max_suggestions(3)
            .build_checked()
            .unwrap();

        assert_eq!(request.text(), "helo world");
        assert_eq!(request.locale(), "en-US");
        assert!(request.checks_spelling());
        assert!(request.checks_grammar());
        assert!(request.autocorrects());
        assert_eq!(
            request.custom_words(),
            &["Kael".to_string(), "GPUI".to_string()]
        );
        assert_eq!(request.max_suggestions(), 3);

        assert!(
            TextCheckingRequestBuilder::new("")
                .locale("en-US")
                .validate()
                .is_err()
        );
        assert!(
            TextCheckingRequestBuilder::new("hello")
                .locale("C")
                .validate()
                .is_err()
        );
        assert!(
            TextCheckingRequestBuilder::new("hello")
                .check_spelling(false)
                .check_grammar_enabled(false)
                .autocorrect_enabled(false)
                .validate()
                .is_err()
        );
        assert!(
            TextCheckingRequestBuilder::new("hello")
                .custom_word(" Kael")
                .validate()
                .is_err()
        );
        assert!(
            TextCheckingRequestBuilder::new("hello")
                .custom_words(["Kael", "kael"])
                .validate()
                .is_err()
        );
        assert!(
            TextCheckingRequestBuilder::new("hello")
                .max_suggestions(21)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_text_checking_request_uses_checked_builder() {
        let cx = TestAppContext::single();
        let locale = LocaleSnapshotBuilder::new()
            .use_system_environment(false)
            .locale("fr_FR")
            .build_checked()
            .unwrap();

        let request = cx
            .read(|app| {
                app.text_checking_request_checked(
                    TextCheckingRequestBuilder::new("bonjour")
                        .locale_snapshot(&locale)
                        .custom_word("Kael"),
                )
            })
            .unwrap();

        assert_eq!(request.locale(), "fr-FR");
        assert_eq!(request.custom_words(), &["Kael".to_string()]);
    }

    #[test]
    fn location_request_builder_validates_runtime_policy() {
        let request = LocationRequest::builder("Show nearby workspaces.")
            .high_accuracy()
            .timeout(Duration::from_secs(30))
            .maximum_age(Duration::from_secs(60))
            .build_checked()
            .unwrap();

        assert_eq!(request.purpose(), "Show nearby workspaces.");
        assert_eq!(request.accuracy(), LocationAccuracy::High);
        assert_eq!(request.timeout(), Duration::from_secs(30));
        assert_eq!(request.maximum_age(), Duration::from_secs(60));
        assert_eq!(request.required_capability(), Capability::Location);
        assert_eq!(
            request.privacy_permission().build_checked().unwrap().kind(),
            AppPrivacyPermissionKind::Location
        );

        assert!(LocationRequestBuilder::new("").validate().is_err());
        assert!(LocationRequestBuilder::new(" nearby").validate().is_err());
        assert!(
            LocationRequestBuilder::new("nearby")
                .timeout(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            LocationRequestBuilder::new("nearby")
                .timeout(Duration::from_secs(121))
                .validate()
                .is_err()
        );
        assert!(
            LocationRequestBuilder::new("nearby")
                .maximum_age(Duration::from_secs(24 * 60 * 60 + 1))
                .validate()
                .is_err()
        );
        assert!(
            LocationRequestBuilder::new("nearby")
                .high_accuracy()
                .allow_background()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_location_request_uses_checked_builder() {
        let cx = TestAppContext::single();
        let request = cx
            .read(|app| {
                app.location_request_checked(
                    LocationRequestBuilder::new("Find nearby projects.")
                        .coarse()
                        .allow_background()
                        .maximum_age(Duration::from_secs(300)),
                )
            })
            .unwrap();

        assert_eq!(request.accuracy(), LocationAccuracy::Coarse);
        assert!(request.allows_background());
    }

    #[test]
    fn device_access_request_builder_validates_runtime_policy() {
        let usb = DeviceAccessRequest::usb("Read measurements from a USB scale.")
            .vendor_product(0x1234, 0xabcd)
            .timeout(Duration::from_secs(20))
            .build_checked()
            .unwrap();
        assert_eq!(usb.kind(), DeviceAccessKind::Usb);
        assert_eq!(usb.vendor_id(), Some(0x1234));
        assert_eq!(usb.product_id(), Some(0xabcd));
        assert_eq!(usb.required_capability(), Capability::UsbDevice);
        assert_eq!(
            usb.privacy_permission().build_checked().unwrap().kind(),
            AppPrivacyPermissionKind::UsbDevices
        );

        let bluetooth = DeviceAccessRequest::bluetooth("Pair with a heart-rate strap.")
            .service_uuid("180D")
            .allow_background()
            .build_checked()
            .unwrap();
        assert_eq!(bluetooth.kind(), DeviceAccessKind::Bluetooth);
        assert_eq!(bluetooth.service_uuid(), Some("180D"));
        assert!(bluetooth.allows_background());
        assert_eq!(bluetooth.required_capability(), Capability::Bluetooth);
        assert_eq!(
            bluetooth
                .privacy_permission()
                .build_checked()
                .unwrap()
                .kind(),
            AppPrivacyPermissionKind::Bluetooth
        );

        assert!(DeviceAccessRequestBuilder::usb("").validate().is_err());
        assert!(
            DeviceAccessRequestBuilder::usb("scale")
                .product_id(0xabcd)
                .validate()
                .is_err()
        );
        assert!(
            DeviceAccessRequestBuilder::usb("scale")
                .service_uuid("180D")
                .validate()
                .is_err()
        );
        assert!(
            DeviceAccessRequestBuilder::serial("reader")
                .vendor_id(0x1234)
                .validate()
                .is_err()
        );
        assert!(
            DeviceAccessRequestBuilder::serial("reader")
                .port_name_hint(" tty.usbserial")
                .validate()
                .is_err()
        );
        assert!(
            DeviceAccessRequestBuilder::bluetooth("strap")
                .service_uuid("not-a-uuid")
                .validate()
                .is_err()
        );
        assert!(
            DeviceAccessRequestBuilder::bluetooth("strap")
                .timeout(Duration::from_secs(121))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_device_access_request_uses_checked_builder() {
        let cx = TestAppContext::single();
        let request = cx
            .read(|app| {
                app.device_access_request_checked(
                    DeviceAccessRequestBuilder::serial("Connect to a CNC controller.")
                        .port_name_hint("tty.usbserial"),
                )
            })
            .unwrap();

        assert_eq!(request.kind(), DeviceAccessKind::Serial);
        assert_eq!(request.port_name_hint(), Some("tty.usbserial"));
        assert_eq!(request.required_capability(), Capability::SerialPort);
    }

    #[test]
    fn process_memory_metrics_report_support_state() {
        let unsupported = super::ProcessMemoryMetrics::unsupported();
        assert_eq!(unsupported.resident_set_bytes(), None);
        assert_eq!(unsupported.virtual_memory_bytes(), None);
        assert_eq!(unsupported.source(), None);
        assert!(!unsupported.is_supported());

        let sampled = super::current_process_memory_metrics();
        if sampled.is_supported() {
            assert!(sampled.source().is_some());
            assert!(
                sampled.resident_set_bytes().is_some() || sampled.virtual_memory_bytes().is_some()
            );
        }
    }

    #[test]
    fn current_process_metrics_include_runtime_metadata() {
        let cx = TestAppContext::single();

        let snapshot = cx.read(|app| app.current_process_metrics());

        assert_eq!(snapshot.process_id(), std::process::id());
        assert_eq!(snapshot.window_count(), 0);
        assert!(snapshot.uptime() < Duration::from_secs(60));
        assert!(snapshot.current_dir().is_some());
        assert!(snapshot.memory().source().is_some() || !snapshot.memory().is_supported());
        assert_eq!(
            snapshot.resident_set_bytes(),
            snapshot.memory().resident_set_bytes()
        );
        assert_eq!(
            snapshot.virtual_memory_bytes(),
            snapshot.memory().virtual_memory_bytes()
        );
    }

    #[test]
    fn app_resource_budget_builder_validates_thresholds() {
        let budget = AppResourceBudgetBuilder::new()
            .max_resident_set_bytes(256 * 1024 * 1024)
            .max_virtual_memory_bytes(2 * 1024 * 1024 * 1024)
            .max_windows(4)
            .max_uptime(Duration::from_secs(60 * 60))
            .require_memory_metrics()
            .warn_when_power_constrained()
            .build_checked()
            .unwrap();

        assert_eq!(budget.max_resident_set_bytes(), Some(256 * 1024 * 1024));
        assert_eq!(
            budget.max_virtual_memory_bytes(),
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(budget.max_windows(), Some(4));
        assert_eq!(budget.max_uptime(), Some(Duration::from_secs(60 * 60)));
        assert!(budget.requires_memory_metrics());
        assert!(budget.warns_when_power_constrained());

        assert!(AppResourceBudgetBuilder::new().validate().is_err());
        assert!(
            AppResourceBudgetBuilder::new()
                .max_resident_set_bytes(0)
                .validate()
                .is_err()
        );
        assert!(
            AppResourceBudgetBuilder::new()
                .max_virtual_memory_bytes(0)
                .validate()
                .is_err()
        );
        assert!(
            AppResourceBudgetBuilder::new()
                .max_windows(0)
                .validate()
                .is_err()
        );
        assert!(
            AppResourceBudgetBuilder::new()
                .max_uptime(Duration::ZERO)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_resource_budget_evaluation_reports_pass_and_failures() {
        let cx = TestAppContext::single();

        let passing = cx
            .read(|app| {
                app.evaluate_resource_budget_checked(
                    AppResourceBudgetBuilder::new()
                        .max_windows(1)
                        .max_uptime(Duration::from_secs(60)),
                )
            })
            .unwrap();
        assert!(passing.is_within_budget());
        assert!(passing.issues().is_empty());
        assert_eq!(passing.summary(), "resource budget ok");
        assert_eq!(passing.metrics().process_id(), std::process::id());
        assert_eq!(passing.runtime().window_count(), 0);
        assert_eq!(passing.budget().max_windows(), Some(1));

        let failing = cx
            .read(|app| {
                app.evaluate_resource_budget_checked(
                    AppResourceBudgetBuilder::new().max_uptime(Duration::from_nanos(1)),
                )
            })
            .unwrap();
        assert!(!failing.is_within_budget());
        assert!(
            failing
                .issues()
                .iter()
                .any(|issue| issue.kind() == AppResourceBudgetIssueKind::UptimeExceeded)
        );
        assert!(failing.summary().contains("uptime"));
    }

    #[test]
    fn app_resource_budget_can_require_memory_metrics() {
        let cx = TestAppContext::single();

        let evaluation = cx
            .read(|app| {
                app.evaluate_resource_budget_checked(
                    AppResourceBudgetBuilder::new()
                        .max_resident_set_bytes(u64::MAX)
                        .require_memory_metrics(),
                )
            })
            .unwrap();

        if evaluation.metrics().memory().is_supported() {
            assert!(!evaluation.missing_required_metrics());
        } else {
            assert!(evaluation.missing_required_metrics());
        }
    }

    #[test]
    fn support_diagnostics_default_is_privacy_safe() {
        let cx = TestAppContext::single();

        let snapshot = cx
            .read(|app| app.support_diagnostics_checked(SupportDiagnosticsBuilder::new()))
            .unwrap();

        assert!(snapshot.metadata().is_none());
        assert!(snapshot.app_paths().is_none());
        assert!(snapshot.launch().args().is_empty());
        assert!(snapshot.launch().environment().is_empty());
        assert!(snapshot.included_environment_keys().is_empty());
        assert_eq!(snapshot.process().process_id(), std::process::id());
        assert_eq!(snapshot.launch().process_id(), std::process::id());
        assert!(!snapshot.locale().locale().is_empty());
        assert!(!snapshot.os().name.is_empty());

        let text = snapshot.to_text();
        assert!(text.contains("Kael diagnostics"));
        assert!(text.contains("Args captured: 0"));
        assert!(text.contains("Environment keys: none"));
    }

    #[test]
    fn support_diagnostics_can_include_metadata_paths_and_args() {
        let cx = TestAppContext::single();

        let snapshot = cx
            .read(|app| {
                app.support_diagnostics_checked(
                    SupportDiagnosticsBuilder::new()
                        .metadata(
                            AppMetadataBuilder::new("Kael Studio")
                                .version("1.2.3")
                                .build("abc123")
                                .identifier("com.example.kael"),
                        )
                        .include_launch_args()
                        .app_paths(AppPathBuilder::new("com.example.kael").role(AppPathRole::Temp))
                        .locale(
                            LocaleSnapshotBuilder::new()
                                .use_system_environment(false)
                                .locale("en_US"),
                        ),
                )
            })
            .unwrap();

        assert_eq!(snapshot.metadata().unwrap().name(), "Kael Studio");
        assert_eq!(snapshot.locale().locale(), "en-US");
        assert!(!snapshot.launch().args().is_empty());
        assert!(
            snapshot
                .app_paths()
                .unwrap()
                .temp_dir()
                .unwrap()
                .ends_with("com.example.kael")
        );

        let text = snapshot.to_text();
        assert!(text.contains("App: Kael Studio 1.2.3"));
        assert!(text.contains("Build: abc123"));
        assert!(text.contains("App paths: com.example.kael"));
    }

    #[test]
    fn support_diagnostics_rejects_side_effectful_app_paths() {
        let builder = SupportDiagnosticsBuilder::new().app_paths(
            AppPathBuilder::new("com.example.kael")
                .role(AppPathRole::Temp)
                .create_dirs(),
        );

        assert!(builder.validate().is_err());
    }

    #[test]
    fn support_diagnostics_validates_environment_allowlist() {
        assert!(
            SupportDiagnosticsBuilder::new()
                .environment_key("APP_CHANNEL")
                .validate()
                .is_ok()
        );
        assert!(
            SupportDiagnosticsBuilder::new()
                .environment_key("BAD=VALUE")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn restart_path_builder_validates_restart_binary() {
        let root =
            std::env::temp_dir().join(format!("kael_restart_path_builder_{}", std::process::id()));
        let nested = root.join("bin");
        let binary = nested.join("app");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(&binary, "binary").unwrap();

        let relative_binary = nested.join("..").join("bin").join("app");
        let path = RestartPathBuilder::new(relative_binary)
            .canonicalize()
            .build_checked()
            .unwrap();
        assert_eq!(path, std::fs::canonicalize(&binary).unwrap());

        let permissive = RestartPathBuilder::new(root.join("missing"))
            .allow_missing()
            .build_checked()
            .unwrap();
        assert_eq!(permissive, root.join("missing"));

        assert!(RestartPathBuilder::new(PathBuf::new()).validate().is_err());
        assert!(
            RestartPathBuilder::new(root.join("missing"))
                .validate()
                .is_err()
        );
        assert!(
            RestartPathBuilder::new(&nested)
                .require_existing_file()
                .validate()
                .is_err()
        );
        assert!(RestartPathBuilder::current_exe().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn set_restart_path_checked_validates_before_platform_call() {
        let cx = TestAppContext::single();
        let root =
            std::env::temp_dir().join(format!("kael_restart_path_checked_{}", std::process::id()));
        let binary = root.join("app");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&binary, "binary").unwrap();

        let stored = cx
            .update(|app| app.set_restart_path_checked(RestartPathBuilder::new(&binary)))
            .unwrap();
        assert_eq!(stored, binary);
        assert!(
            cx.update(|app| {
                app.set_restart_path_checked(RestartPathBuilder::new(root.join("missing")))
            })
            .is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dock_badge_builder_validates_labels() {
        let clear = DockBadgeBuilder::clear();
        assert!(clear.validate().is_ok());
        assert!(clear.is_clear());
        assert_eq!(clear.label_text(), None);

        let count = DockBadgeBuilder::count(42);
        assert!(count.validate().is_ok());
        assert_eq!(count.label_text(), Some("42"));
        assert_eq!(count.build_checked().unwrap(), Some("42".to_string()));

        let status = DockBadgeBuilder::label("syncing");
        assert!(status.validate().is_ok());
        assert_eq!(status.label_text(), Some("syncing"));

        assert!(DockBadgeBuilder::label("").validate().is_err());
        assert!(DockBadgeBuilder::label(" ").validate().is_err());
        assert!(DockBadgeBuilder::label(" 3").validate().is_err());
        assert!(DockBadgeBuilder::label("3 ").validate().is_err());
        assert!(DockBadgeBuilder::label("line\nbreak").validate().is_err());
        assert!(
            DockBadgeBuilder::label("this-label-is-too-long")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn set_dock_badge_checked_validates_before_platform_call() {
        let cx = TestAppContext::single();

        assert!(
            cx.read(|app| app.set_dock_badge_checked(DockBadgeBuilder::count(7)))
                .is_ok()
        );
        assert!(
            cx.read(|app| app.set_dock_badge_checked(DockBadgeBuilder::clear()))
                .is_ok()
        );
        assert!(
            cx.read(|app| app.set_dock_badge_checked(DockBadgeBuilder::label(" bad")))
                .is_err()
        );
    }

    #[test]
    fn tray_tooltip_builder_validates_status_text() {
        let clear = TrayTooltipBuilder::clear();
        assert!(clear.validate().is_ok());
        assert!(clear.is_clear());
        assert_eq!(clear.tooltip_text(), None);

        let status = TrayTooltipBuilder::status("Sync complete");
        assert!(status.validate().is_ok());
        assert_eq!(status.tooltip_text(), Some("Sync complete"));
        assert_eq!(
            status.build_checked().unwrap(),
            Some("Sync complete".to_string())
        );

        assert!(TrayTooltipBuilder::text("").validate().is_err());
        assert!(TrayTooltipBuilder::text(" ").validate().is_err());
        assert!(TrayTooltipBuilder::text(" syncing").validate().is_err());
        assert!(TrayTooltipBuilder::text("syncing ").validate().is_err());
        assert!(TrayTooltipBuilder::text("line\nbreak").validate().is_err());
        assert!(
            TrayTooltipBuilder::text("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn set_tray_tooltip_checked_validates_before_platform_call() {
        let cx = TestAppContext::single();

        assert!(
            cx.read(|app| app.set_tray_tooltip_checked(TrayTooltipBuilder::status("Online")))
                .is_ok()
        );
        assert!(
            cx.read(|app| app.set_tray_tooltip_checked(TrayTooltipBuilder::clear()))
                .is_ok()
        );
        assert!(
            cx.read(|app| app.set_tray_tooltip_checked(TrayTooltipBuilder::text(" bad")))
                .is_err()
        );
    }

    #[test]
    fn tray_app_builder_validates_menu_tooltip_and_background_mode() {
        let config = TrayAppBuilder::new()
            .action("Show Window", "show")
            .separator()
            .toggle("Pause Sync", false, "pause-sync")
            .status_tooltip("Sync running")
            .panel()
            .build_checked()
            .unwrap();

        assert_eq!(config.menu().len(), 3);
        assert_eq!(config.tooltip(), Some("Sync running"));
        assert!(config.panel_mode());
        assert!(config.keep_alive_without_windows());

        assert!(TrayAppBuilder::new().validate().is_err());
        assert!(
            TrayAppBuilder::new()
                .action("Show", "show")
                .status_tooltip(" bad")
                .validate()
                .is_err()
        );
        assert!(
            TrayAppBuilder::new()
                .action("Show", "duplicate")
                .action("Hide", "duplicate")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn configure_tray_app_checked_applies_all_tray_state() {
        let cx = TestAppContext::single();

        let config = cx
            .update(|app| {
                app.configure_tray_app_checked(
                    TrayAppBuilder::new()
                        .action("Show Window", "show")
                        .separator()
                        .action("Quit", "quit")
                        .status_tooltip("Sync running")
                        .panel()
                        .keep_alive_without_windows(true),
                )
            })
            .unwrap();

        assert_eq!(config.menu().len(), 3);
        assert_eq!(cx.tray_menu(), config.menu());
        assert_eq!(cx.tray_tooltip(), "Sync running");
        assert!(cx.tray_panel_mode());
        assert!(cx.keep_alive_without_windows());

        assert!(
            cx.update(|app| {
                app.configure_tray_app_checked(
                    TrayAppBuilder::new()
                        .action("Show Window", "show")
                        .status_tooltip(" bad"),
                )
            })
            .is_err()
        );
    }

    #[test]
    fn runtime_snapshot_reports_app_lifecycle_state() {
        let cx = TestAppContext::single();

        let initial = cx.read(|app| app.runtime_snapshot());
        assert_eq!(initial.process_id(), ProcessId(0));
        assert_eq!(initial.window_count(), 0);
        assert!(!initial.keep_alive_without_windows());
        assert!(!initial.is_background_runtime());
        assert_eq!(initial.quit_cleanup_timeout(), SHUTDOWN_TIMEOUT);
        assert!(!initial.is_quitting());
        assert_eq!(initial.network_status(), NetworkStatus::Online);
        assert_eq!(initial.power().power_mode(), PowerMode::Performance);
        assert!(initial.theme().is_light());

        cx.update(|app| {
            app.set_keep_alive_without_windows(true);
        });
        let background = cx.read(|app| app.runtime_snapshot());
        assert!(background.keep_alive_without_windows());
        assert!(background.is_background_runtime());

        cx.update(|app| {
            app.configure_lifecycle_policy_checked(
                AppLifecyclePolicyBuilder::new()
                    .quit_when_all_windows_close()
                    .quit_cleanup_timeout(Duration::from_millis(300)),
            )
        })
        .unwrap();
        let foreground = cx.read(|app| app.runtime_snapshot());
        assert!(!foreground.keep_alive_without_windows());
        assert!(!foreground.is_background_runtime());
        assert_eq!(
            foreground.quit_cleanup_timeout(),
            Duration::from_millis(300)
        );
        assert!(foreground.uptime() < Duration::from_secs(60));
    }

    #[test]
    fn app_lifecycle_command_validates_activation_and_terminal_commands() {
        let activate =
            AppLifecycleCommand::activate_with_options(true).reason("Show existing project window");
        assert!(activate.validate().is_ok());
        assert_eq!(
            activate.kind(),
            AppLifecycleCommandKind::Activate {
                ignoring_other_apps: true
            }
        );
        assert_eq!(activate.reason_text(), Some("Show existing project window"));
        assert!(!activate.is_terminal());

        let quit = AppLifecycleCommand::quit("User selected Quit");
        assert!(quit.validate().is_ok());
        assert!(quit.is_terminal());

        let restart = AppLifecycleCommand::restart("Apply update");
        assert!(restart.validate().is_ok());
        assert!(restart.is_terminal());

        assert!(AppLifecycleCommand::quit("").validate().is_err());
        assert!(
            AppLifecycleCommand::restart(" Apply update")
                .validate()
                .is_err()
        );
        assert!(
            AppLifecycleCommand::hide()
                .reason("Line one\nLine two")
                .validate()
                .is_ok()
        );
        assert!(
            AppLifecycleCommand::hide()
                .reason("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn perform_lifecycle_command_checked_dispatches_safe_commands() {
        let cx = TestAppContext::single();

        let command = cx
            .update(|app| {
                app.perform_lifecycle_command_checked(AppLifecycleCommand::activate_with_options(
                    false,
                ))
            })
            .unwrap();
        assert_eq!(
            command.kind(),
            AppLifecycleCommandKind::Activate {
                ignoring_other_apps: false
            }
        );

        let command = cx
            .update(|app| app.perform_lifecycle_command_checked(AppLifecycleCommand::hide()))
            .unwrap();
        assert_eq!(command.kind(), AppLifecycleCommandKind::Hide);

        let command = cx
            .update(|app| {
                app.perform_lifecycle_command_checked(AppLifecycleCommand::hide_other_apps())
            })
            .unwrap();
        assert_eq!(command.kind(), AppLifecycleCommandKind::HideOtherApps);

        let command = cx
            .update(|app| {
                app.perform_lifecycle_command_checked(AppLifecycleCommand::unhide_other_apps())
            })
            .unwrap();
        assert_eq!(command.kind(), AppLifecycleCommandKind::UnhideOtherApps);

        assert!(
            cx.update(|app| {
                app.perform_lifecycle_command_checked(AppLifecycleCommand::restart(""))
            })
            .is_err()
        );
    }

    #[test]
    fn app_window_capture_request_builder_validates_visual_capture_policy() {
        let request =
            AppWindowCaptureRequest::focused_window("Capture visual regression evidence.")
                .rgba()
                .include_window_chrome()
                .include_cursor()
                .max_dimensions(1920, 1080)
                .max_pixels(2_073_600)
                .build_checked()
                .unwrap();

        assert_eq!(request.purpose(), "Capture visual regression evidence.");
        assert_eq!(request.target(), AppWindowCaptureTarget::FocusedWindow);
        assert_eq!(request.format(), AppWindowCaptureFormat::Rgba);
        assert!(request.includes_window_chrome());
        assert!(request.includes_cursor());
        assert!(!request.allows_occluded());
        assert_eq!(request.required_capability(), None);

        let occluded =
            AppWindowCaptureRequestBuilder::focused_window("Capture hidden error dialog.")
                .allow_occluded()
                .build_checked()
                .unwrap();
        assert!(occluded.allows_occluded());
        assert_eq!(
            occluded.required_capability(),
            Some(Capability::ScreenCapture)
        );

        assert!(
            AppWindowCaptureRequestBuilder::focused_window("")
                .validate()
                .is_err()
        );
        assert!(
            AppWindowCaptureRequestBuilder::focused_window("Capture")
                .max_dimensions(0, 1080)
                .validate()
                .is_err()
        );
        assert!(
            AppWindowCaptureRequestBuilder::focused_window("Capture")
                .max_dimensions(16_385, 1080)
                .validate()
                .is_err()
        );
        assert!(
            AppWindowCaptureRequestBuilder::focused_window("Capture")
                .max_pixels(0)
                .validate()
                .is_err()
        );
        assert!(
            AppWindowCaptureRequestBuilder::visible_app_windows("Capture all windows.")
                .include_cursor()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn app_window_capture_request_checked_uses_builder_validation() {
        let cx = TestAppContext::single();
        let request = cx
            .read(|app| {
                app.app_window_capture_request_checked(
                    AppWindowCaptureRequestBuilder::visible_app_windows(
                        "Capture support screenshot bundle.",
                    )
                    .png()
                    .unlimited_dimensions()
                    .max_pixels(8_000_000),
                )
            })
            .unwrap();

        assert_eq!(request.target(), AppWindowCaptureTarget::VisibleAppWindows);
        assert_eq!(request.format(), AppWindowCaptureFormat::Png);
        assert_eq!(request.max_width_px(), None);
        assert_eq!(request.max_height_px(), None);
        assert_eq!(request.max_pixels(), Some(8_000_000));

        assert!(
            cx.read(|app| {
                app.app_window_capture_request_checked(
                    AppWindowCaptureRequestBuilder::visible_app_windows("Capture").include_cursor(),
                )
            })
            .is_err()
        );
    }

    #[test]
    fn app_lifecycle_policy_builder_validates_shutdown_policy() {
        let policy = AppLifecyclePolicyBuilder::new()
            .keep_alive_without_windows()
            .quit_cleanup_timeout(Duration::from_millis(250))
            .reason("tray background sync")
            .build_checked()
            .unwrap();

        assert!(policy.keep_alive_without_windows());
        assert_eq!(
            policy.window_close_behavior(),
            super::WindowCloseBehavior::KeepAliveWithoutWindows
        );
        assert_eq!(policy.quit_cleanup_timeout(), Duration::from_millis(250));
        assert_eq!(policy.reason(), Some("tray background sync"));

        assert!(
            AppLifecyclePolicyBuilder::new()
                .quit_cleanup_timeout(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            AppLifecyclePolicyBuilder::new()
                .quit_cleanup_timeout(Duration::from_secs(31))
                .validate()
                .is_err()
        );
        assert!(
            AppLifecyclePolicyBuilder::new()
                .reason(" bad")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn configure_lifecycle_policy_checked_applies_platform_state() {
        let cx = TestAppContext::single();

        let policy = cx
            .update(|app| {
                app.configure_lifecycle_policy_checked(
                    AppLifecyclePolicyBuilder::new()
                        .keep_alive_without_windows()
                        .quit_cleanup_timeout(Duration::from_millis(500)),
                )
            })
            .unwrap();
        assert!(policy.keep_alive_without_windows());
        assert!(cx.keep_alive_without_windows());
        assert_eq!(
            cx.read(|app| app.quit_cleanup_timeout()),
            Duration::from_millis(500)
        );

        let policy = cx
            .update(|app| {
                app.configure_lifecycle_policy_checked(
                    AppLifecyclePolicyBuilder::new()
                        .quit_when_all_windows_close()
                        .quit_cleanup_timeout(Duration::from_millis(100)),
                )
            })
            .unwrap();
        assert!(!policy.keep_alive_without_windows());
        assert!(!cx.keep_alive_without_windows());
        assert_eq!(
            cx.read(|app| app.quit_cleanup_timeout()),
            Duration::from_millis(100)
        );

        assert!(
            cx.update(|app| {
                app.configure_lifecycle_policy_checked(
                    AppLifecyclePolicyBuilder::new().quit_cleanup_timeout(Duration::ZERO),
                )
            })
            .is_err()
        );
    }

    #[test]
    fn biometric_auth_builder_validates_prompt_reason() {
        assert!(
            BiometricAuthBuilder::new("Unlock your vault")
                .validate()
                .is_ok()
        );
        assert!(BiometricAuthBuilder::unlock_vault().validate().is_ok());
        assert_eq!(
            BiometricAuthBuilder::unlock_vault().reason(),
            "Unlock your vault"
        );
        assert!(BiometricAuthBuilder::approve_payment().validate().is_ok());
        assert_eq!(
            BiometricAuthBuilder::approve_payment().reason(),
            "Approve payment"
        );
        assert!(BiometricAuthBuilder::new("  ").validate().is_err());
        assert!(BiometricAuthBuilder::new(" Unlock").validate().is_err());
        assert!(BiometricAuthBuilder::new("Unlock ").validate().is_err());
        assert!(
            BiometricAuthBuilder::new("Unlock\nvault")
                .validate()
                .is_err()
        );
        assert!(
            BiometricAuthBuilder::new("Unlock\0vault")
                .validate()
                .is_err()
        );
        assert!(
            BiometricAuthBuilder::new("a".repeat(257))
                .validate()
                .is_err()
        );

        let prompt = BiometricAuthBuilder::new("Retry biometric unlock").allow_unavailable();
        assert_eq!(prompt.reason(), "Retry biometric unlock");
        assert!(!prompt.requires_available());
        assert_eq!(
            prompt.build_checked().unwrap(),
            ("Retry biometric unlock".to_string(), false)
        );
    }

    #[test]
    fn authenticate_biometric_with_skips_unavailable_prompt_by_default() {
        let cx = TestAppContext::single();
        let called = Arc::new(Mutex::new(false));

        let request = cx
            .read(|app| {
                app.authenticate_biometric_with(BiometricAuthBuilder::new("Unlock secure notes"), {
                    let called = called.clone();
                    move |_| {
                        *called.lock().unwrap() = true;
                    }
                })
            })
            .unwrap();

        assert_eq!(request.status(), BiometricStatus::Unavailable);
        assert!(!request.prompted());
        assert_eq!(request.reason(), "Unlock secure notes");
        assert!(!*called.lock().unwrap());
        assert!(cx.biometric_auth_reasons().is_empty());
    }

    #[test]
    fn authenticate_biometric_with_routes_available_prompt() {
        let cx = TestAppContext::single();
        cx.set_biometric_status(BiometricStatus::Available(BiometricKind::Fingerprint));
        cx.set_biometric_auth_success(true);
        let result = Arc::new(Mutex::new(None));

        let request = cx
            .read(|app| {
                app.authenticate_biometric_with(BiometricAuthBuilder::new("Approve payment"), {
                    let result = result.clone();
                    move |success| {
                        *result.lock().unwrap() = Some(success);
                    }
                })
            })
            .unwrap();

        assert_eq!(
            request.status(),
            BiometricStatus::Available(BiometricKind::Fingerprint)
        );
        assert!(request.prompted());
        assert_eq!(request.reason(), "Approve payment");
        assert_eq!(*result.lock().unwrap(), Some(true));
        assert_eq!(
            cx.biometric_auth_reasons(),
            vec!["Approve payment".to_string()]
        );
    }

    #[test]
    fn credential_builder_validates_required_fields() {
        assert!(
            CredentialBuilder::new("")
                .username("user")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new(" ")
                .username("user")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new(" service")
                .username("user")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new("service")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new("service")
                .username("user")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new("service")
                .username(" ")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new("service")
                .username("user ")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new("service\0")
                .username("user")
                .password("secret")
                .validate()
                .is_err()
        );
        assert!(
            CredentialBuilder::new("service")
                .username("user\nname")
                .password("secret")
                .validate()
                .is_err()
        );

        let credential = CredentialBuilder::new("https://example.com")
            .username("ada")
            .password("correct horse");

        assert!(credential.validate().is_ok());
        assert_eq!(credential.service(), "https://example.com");
        assert_eq!(credential.configured_username(), Some("ada"));
        assert_eq!(
            credential.configured_secret(),
            Some("correct horse".as_bytes())
        );
    }

    #[test]
    fn credential_service_builder_validates_read_delete_keys() {
        assert!(CredentialServiceBuilder::new("").validate().is_err());
        assert!(
            CredentialServiceBuilder::new(" service")
                .validate()
                .is_err()
        );
        assert!(
            CredentialServiceBuilder::new("service ")
                .validate()
                .is_err()
        );
        assert!(
            CredentialServiceBuilder::new("service\0")
                .validate()
                .is_err()
        );
        assert!(
            CredentialServiceBuilder::new("service\nname")
                .validate()
                .is_err()
        );

        let service = CredentialServiceBuilder::new("https://example.com");
        assert!(service.validate().is_ok());
        assert_eq!(service.service(), "https://example.com");
        assert_eq!(service.build().unwrap(), "https://example.com");
    }

    #[test]
    fn secure_credential_helpers_round_trip_through_platform() {
        let cx = TestAppContext::single();

        let write = cx
            .read(|app| {
                app.write_secure_credential(
                    CredentialBuilder::new("https://example.com")
                        .username("ada")
                        .password("correct horse"),
                )
            })
            .unwrap();
        cx.background_executor.block(write).unwrap();

        let stored = cx
            .background_executor
            .block(
                cx.read(|app| {
                    app.read_secure_credential_checked(CredentialServiceBuilder::new(
                        "https://example.com",
                    ))
                })
                .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(stored.username(), "ada");
        assert_eq!(stored.secret(), b"correct horse");

        cx.background_executor
            .block(
                cx.read(|app| {
                    app.delete_secure_credential_checked(CredentialServiceBuilder::new(
                        "https://example.com",
                    ))
                })
                .unwrap(),
            )
            .unwrap();

        let missing = cx
            .background_executor
            .block(cx.read(|app| app.read_secure_credential("https://example.com")))
            .unwrap();
        assert!(missing.is_none());

        assert!(
            cx.read(
                |app| app.read_secure_credential_checked(CredentialServiceBuilder::new(
                    " https://example.com"
                ))
            )
            .is_err()
        );
        assert!(
            cx.read(
                |app| app.delete_secure_credential_checked(CredentialServiceBuilder::new(
                    "https://example.com\0"
                ))
            )
            .is_err()
        );
    }

    #[test]
    fn power_save_blocker_builder_preserves_intent() {
        let blocker = PowerSaveBlockerBuilder::prevent_display_sleep().reason("video playback");

        assert_eq!(blocker.kind(), PowerSaveBlockerKind::PreventDisplaySleep);
        assert_eq!(blocker.configured_reason(), Some("video playback"));
        assert!(blocker.validate().is_ok());
        assert!(
            PowerSaveBlockerBuilder::prevent_display_sleep()
                .reason(" ")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn start_power_save_blocker_with_returns_stoppable_handle() {
        let cx = TestAppContext::single();

        let handle = cx
            .read(|app| {
                app.start_power_save_blocker_with(
                    PowerSaveBlockerBuilder::prevent_display_sleep().reason("video playback"),
                )
            })
            .unwrap();

        assert_eq!(handle.id(), 1);
        assert_eq!(handle.kind(), PowerSaveBlockerKind::PreventDisplaySleep);
        assert_eq!(handle.reason(), Some("video playback"));
        assert_eq!(
            cx.power_save_blockers(),
            vec![(1, PowerSaveBlockerKind::PreventDisplaySleep)]
        );

        cx.read(|app| handle.stop(app));

        assert!(cx.power_save_blockers().is_empty());
    }

    #[test]
    fn start_power_save_blocker_checked_validates_reason() {
        let cx = TestAppContext::single();

        let handle = cx
            .read(|app| {
                app.start_power_save_blocker_checked(
                    PowerSaveBlockerBuilder::prevent_app_suspension().reason("background sync"),
                )
            })
            .unwrap()
            .unwrap();

        assert_eq!(handle.id(), 1);
        assert_eq!(handle.kind(), PowerSaveBlockerKind::PreventAppSuspension);
        assert_eq!(handle.reason(), Some("background sync"));

        let error = cx.read(|app| {
            app.start_power_save_blocker_checked(
                PowerSaveBlockerBuilder::prevent_display_sleep().reason(""),
            )
        });

        assert!(error.is_err());
    }

    #[test]
    fn system_power_snapshot_reports_adaptive_state() {
        let cx = TestAppContext::single();
        cx.set_power_mode(PowerMode::LowPower);
        cx.set_reduce_motion(true);

        let snapshot = cx.read(|app| app.system_power_snapshot());

        assert_eq!(snapshot.power_mode(), PowerMode::LowPower);
        assert!(snapshot.reduce_motion());
        assert_eq!(snapshot.idle_time(), None);
        assert!(snapshot.should_reduce_work());
    }

    #[test]
    fn native_theme_snapshot_reports_appearance_and_adaptation() {
        let snapshot =
            NativeThemeSnapshot::new(WindowAppearance::VibrantDark, true, PowerMode::Performance);

        assert_eq!(snapshot.appearance(), WindowAppearance::VibrantDark);
        assert!(snapshot.is_dark());
        assert!(!snapshot.is_light());
        assert!(snapshot.is_vibrant());
        assert!(snapshot.reduce_motion());
        assert_eq!(snapshot.power_mode(), PowerMode::Performance);
        assert!(snapshot.should_reduce_effects());
        assert_eq!(snapshot.choose("dark", "light"), "dark");

        let low_power =
            NativeThemeSnapshot::new(WindowAppearance::Light, false, PowerMode::LowPower);
        assert!(low_power.is_light());
        assert!(!low_power.is_vibrant());
        assert!(low_power.should_reduce_effects());
        assert_eq!(low_power.choose("dark", "light"), "light");
    }

    #[test]
    fn app_native_theme_snapshot_uses_platform_signals() {
        let cx = TestAppContext::single();
        cx.set_power_mode(PowerMode::LowPower);
        cx.set_reduce_motion(true);

        let snapshot = cx.read(|app| app.native_theme_snapshot());

        assert_eq!(snapshot.appearance(), WindowAppearance::Light);
        assert!(snapshot.is_light());
        assert!(snapshot.reduce_motion());
        assert_eq!(snapshot.power_mode(), PowerMode::LowPower);
        assert!(snapshot.should_reduce_effects());
    }

    #[test]
    fn system_idle_policy_evaluates_known_idle_time() {
        let policy = SystemIdlePolicyBuilder::minutes(5)
            .require_known_idle_time()
            .build_checked()
            .unwrap();

        let idle_snapshot = SystemPowerSnapshot {
            power_mode: PowerMode::Performance,
            reduce_motion: false,
            idle_time: Some(Duration::from_secs(360)),
        };
        let active_snapshot = SystemPowerSnapshot {
            power_mode: PowerMode::Performance,
            reduce_motion: false,
            idle_time: Some(Duration::from_secs(120)),
        };

        let idle = policy.evaluate(&idle_snapshot);
        assert!(idle.is_idle());
        assert_eq!(idle.idle_time(), Some(Duration::from_secs(360)));
        assert_eq!(idle.threshold(), Duration::from_secs(300));

        let active = idle_snapshot.evaluate_idle(
            &SystemIdlePolicyBuilder::minutes(10)
                .build_checked()
                .unwrap(),
        );
        assert!(!active.is_idle());
        assert_eq!(active.idle_time(), Some(Duration::from_secs(360)));

        assert!(!policy.allows(&active_snapshot));
    }

    #[test]
    fn system_idle_policy_handles_unknown_idle_time_explicitly() {
        let snapshot = SystemPowerSnapshot {
            power_mode: PowerMode::Performance,
            reduce_motion: false,
            idle_time: None,
        };

        let conservative = SystemIdlePolicyBuilder::seconds(30)
            .build_checked()
            .unwrap();
        assert_eq!(
            conservative.evaluate(&snapshot),
            SystemIdleEvaluation::Unknown {
                threshold: Duration::from_secs(30),
                treated_as_idle: false
            }
        );
        assert!(!conservative.allows(&snapshot));

        let permissive = SystemIdlePolicyBuilder::seconds(30)
            .treat_unknown_as_idle()
            .build_checked()
            .unwrap();
        assert!(permissive.evaluate(&snapshot).is_idle());

        assert!(SystemIdlePolicyBuilder::seconds(0).build_checked().is_err());
        assert!(
            SystemIdlePolicyBuilder::seconds(30)
                .require_known_idle_time()
                .treat_unknown_as_idle()
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn app_system_idle_evaluation_checked_validates_policy() {
        let cx = TestAppContext::single();

        let evaluation = cx
            .read(|app| {
                app.system_idle_evaluation_checked(
                    SystemIdlePolicyBuilder::seconds(30).treat_unknown_as_idle(),
                )
            })
            .unwrap();

        assert!(evaluation.is_idle());
        assert_eq!(evaluation.idle_time(), None);

        assert!(
            cx.read(|app| app.system_idle_evaluation_checked(SystemIdlePolicyBuilder::seconds(0)))
                .is_err()
        );
    }

    #[test]
    fn system_power_monitor_builder_validates_callbacks() {
        assert!(SystemPowerMonitorBuilder::new().validate().is_err());
        assert!(
            SystemPowerMonitorBuilder::new()
                .on_resume(|_, _| {})
                .validate()
                .is_ok()
        );

        let cx = TestAppContext::single();
        assert!(
            cx.update(|app| app.watch_system_power_checked(SystemPowerMonitorBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn system_power_monitor_routes_events_with_snapshot() {
        let cx = TestAppContext::single();
        let events = Rc::new(RefCell::new(Vec::new()));
        let power_modes = Rc::new(RefCell::new(Vec::new()));
        let resumed = Rc::new(RefCell::new(false));

        let monitor = cx.update(|app| {
            app.watch_system_power_checked(
                SystemPowerMonitorBuilder::new()
                    .on_event({
                        let events = events.clone();
                        move |event, snapshot, _| {
                            events
                                .borrow_mut()
                                .push((event, snapshot.should_reduce_work()));
                        }
                    })
                    .on_power_mode_changed({
                        let power_modes = power_modes.clone();
                        move |snapshot, _| {
                            power_modes.borrow_mut().push(snapshot.power_mode());
                        }
                    })
                    .on_resume({
                        let resumed = resumed.clone();
                        move |_, _| {
                            *resumed.borrow_mut() = true;
                        }
                    }),
            )
            .unwrap()
        });

        assert_eq!(
            monitor.initial_snapshot().power_mode(),
            PowerMode::Performance
        );
        assert!(!monitor.initially_should_reduce_work());

        cx.set_power_mode(PowerMode::LowPower);
        cx.simulate_system_power_event(SystemPowerEvent::PowerModeChanged);
        cx.simulate_system_power_event(SystemPowerEvent::Resume);

        assert_eq!(
            events.borrow().clone(),
            vec![
                (SystemPowerEvent::PowerModeChanged, true),
                (SystemPowerEvent::Resume, true)
            ]
        );
        assert_eq!(power_modes.borrow().clone(), vec![PowerMode::LowPower]);
        assert!(*resumed.borrow());
    }

    #[test]
    fn window_placement_builder_validates_positive_size() {
        assert!(
            WindowPlacementBuilder::new(size(px(320.), px(240.)))
                .validate()
                .is_ok()
        );
        assert!(
            WindowPlacementBuilder::new(size(px(0.), px(240.)))
                .validate()
                .is_err()
        );
        assert!(
            WindowPlacementBuilder::new(size(px(320.), px(0.)))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn resolve_window_placement_centers_on_primary_display() {
        let cx = TestAppContext::single();

        let placement = cx
            .read(|app| {
                app.resolve_window_placement(WindowPlacementBuilder::new(size(px(400.), px(300.))))
            })
            .unwrap();

        assert_eq!(placement.size(), size(px(400.), px(300.)));
        assert_eq!(placement.display_id(), Some(crate::DisplayId(1)));
        assert_eq!(placement.bounds().origin.x, px(760.));
        assert_eq!(placement.bounds().origin.y, px(390.));
    }

    #[test]
    fn resolve_window_placement_handles_primary_display_corners() {
        let cx = TestAppContext::single();

        let placement = cx
            .read(|app| {
                app.resolve_window_placement(
                    WindowPlacementBuilder::new(size(px(320.), px(180.))).bottom_right(px(16.)),
                )
            })
            .unwrap();

        assert_eq!(placement.display_id(), Some(crate::DisplayId(1)));
        assert_eq!(placement.bounds().origin.x, px(1584.));
        assert_eq!(placement.bounds().origin.y, px(884.));
    }

    #[test]
    fn display_query_builder_validates_targets() {
        assert!(DisplayQueryBuilder::all().validate().is_ok());
        assert!(DisplayQueryBuilder::primary().validate().is_ok());
        assert!(DisplayQueryBuilder::cursor().validate().is_ok());
        assert!(
            DisplayQueryBuilder::all()
                .fallback_to_primary()
                .validate()
                .is_err()
        );
        assert!(DisplayQueryBuilder::cursor().falls_back_to_primary());
        assert!(DisplayQueryBuilder::primary().requires_match());
    }

    #[test]
    fn query_displays_checked_returns_primary_and_all_displays() {
        let cx = TestAppContext::single();

        let all = cx
            .read(|app| app.query_displays_checked(DisplayQueryBuilder::all()))
            .unwrap();
        assert!(all.has_match());
        assert_eq!(all.displays().len(), 1);
        assert_eq!(all.first().unwrap().id(), crate::DisplayId(1));
        assert!(all.first().unwrap().is_primary());
        assert_eq!(all.first().unwrap().refresh_rate(), Some(60.0));
        assert_eq!(
            all.first().unwrap().bounds().size,
            size(px(1920.), px(1080.))
        );

        let primary = cx
            .read(|app| app.query_displays_checked(DisplayQueryBuilder::primary()))
            .unwrap();
        assert_eq!(primary.target(), super::DisplayQueryTarget::Primary);
        assert_eq!(primary.first().unwrap().id(), crate::DisplayId(1));
        assert!(primary.first().unwrap().uuid().is_some());
    }

    #[test]
    fn query_displays_checked_handles_id_and_cursor_fallback() {
        let cx = TestAppContext::single();

        let by_id = cx
            .read(|app| {
                app.query_displays_checked(DisplayQueryBuilder::display_id(crate::DisplayId(1)))
            })
            .unwrap();
        assert_eq!(by_id.first().unwrap().id(), crate::DisplayId(1));

        let missing = cx
            .read(|app| {
                app.query_displays_checked(
                    DisplayQueryBuilder::display_id(crate::DisplayId(999)).allow_empty(),
                )
            })
            .unwrap();
        assert!(!missing.has_match());

        assert!(
            cx.read(|app| {
                app.query_displays_checked(DisplayQueryBuilder::display_id(crate::DisplayId(999)))
            })
            .is_err()
        );

        let cursor = cx
            .read(|app| app.query_displays_checked(DisplayQueryBuilder::cursor()))
            .unwrap();
        assert_eq!(cursor.first().unwrap().id(), crate::DisplayId(1));
        assert_eq!(cursor.cursor_position(), None);
    }

    #[test]
    fn window_options_builder_accepts_resolved_placement() {
        let cx = TestAppContext::single();

        let placement = cx
            .read(|app| {
                app.resolve_window_placement(
                    WindowPlacementBuilder::new(size(px(320.), px(180.))).top_left(px(24.)),
                )
            })
            .unwrap();

        let options = WindowOptionsBuilder::new()
            .title("Downloads")
            .placement(&placement)
            .floating()
            .build();

        assert_eq!(
            options.window_bounds,
            Some(WindowBounds::Windowed(placement.bounds()))
        );
        assert_eq!(options.display_id, placement.display_id());
    }

    #[test]
    fn user_attention_builder_preserves_intent() {
        let attention = UserAttentionBuilder::critical().reason("background export failed");

        assert_eq!(attention.attention_type(), AttentionType::Critical);
        assert_eq!(
            attention.configured_reason(),
            Some("background export failed")
        );
        assert!(attention.validate().is_ok());
        assert!(
            UserAttentionBuilder::informational()
                .reason(" ")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn request_user_attention_with_returns_cancellable_request() {
        let cx = TestAppContext::single();

        let request = cx.read(|app| {
            app.request_user_attention_with(
                UserAttentionBuilder::informational().reason("download complete"),
            )
        });

        assert_eq!(request.attention_type(), AttentionType::Informational);
        assert_eq!(request.reason(), Some("download complete"));
        assert_eq!(cx.user_attention(), Some(AttentionType::Informational));

        cx.read(|app| request.cancel(app));

        assert_eq!(cx.user_attention(), None);
        assert_eq!(cx.user_attention_cancel_count(), 1);
    }

    #[test]
    fn request_user_attention_checked_validates_reason() {
        let cx = TestAppContext::single();

        let request = cx
            .read(|app| {
                app.request_user_attention_checked(
                    UserAttentionBuilder::critical().reason("sync failed"),
                )
            })
            .unwrap();

        assert_eq!(request.attention_type(), AttentionType::Critical);
        assert_eq!(request.reason(), Some("sync failed"));
        assert_eq!(cx.user_attention(), Some(AttentionType::Critical));

        let error = cx.read(|app| {
            app.request_user_attention_checked(UserAttentionBuilder::informational().reason(""))
        });

        assert!(error.is_err());
    }

    #[test]
    fn network_status_monitor_reports_initial_status() {
        let cx = TestAppContext::single();

        let monitor = cx.read(|app| app.watch_network_status(NetworkStatusMonitorBuilder::new()));

        assert_eq!(monitor.initial_status(), crate::NetworkStatus::Online);
        assert!(monitor.initially_online());
        assert!(!monitor.initially_offline());
        assert_eq!(cx.network_status(), crate::NetworkStatus::Online);
    }

    #[test]
    fn network_status_monitor_builder_validates_callbacks() {
        assert!(NetworkStatusMonitorBuilder::new().validate().is_err());
        assert!(
            NetworkStatusMonitorBuilder::new()
                .on_change(|_, _| {})
                .validate()
                .is_ok()
        );

        let cx = TestAppContext::single();
        assert!(
            cx.read(|app| app.watch_network_status_checked(NetworkStatusMonitorBuilder::new()))
                .is_err()
        );
    }

    #[test]
    fn network_status_monitor_routes_callbacks() {
        let cx = TestAppContext::single();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let online_count = Rc::new(std::cell::Cell::new(0));
        let offline_count = Rc::new(std::cell::Cell::new(0));

        cx.read(|app| {
            app.watch_network_status_checked(
                NetworkStatusMonitorBuilder::new()
                    .on_change({
                        let changes = changes.clone();
                        move |status, _| changes.borrow_mut().push(status)
                    })
                    .on_online({
                        let online_count = online_count.clone();
                        move |_| online_count.set(online_count.get() + 1)
                    })
                    .on_offline({
                        let offline_count = offline_count.clone();
                        move |_| offline_count.set(offline_count.get() + 1)
                    }),
            )
            .unwrap()
        });

        cx.simulate_network_status_change(crate::NetworkStatus::Offline);
        cx.simulate_network_status_change(crate::NetworkStatus::Online);

        assert_eq!(
            changes.borrow().as_slice(),
            &[crate::NetworkStatus::Offline, crate::NetworkStatus::Online]
        );
        assert_eq!(offline_count.get(), 1);
        assert_eq!(online_count.get(), 1);
    }

    #[test]
    fn capture_manager_uses_app_permission_context() {
        let cx = TestAppContext::single();

        let manager = cx.read(|app| app.capture_manager());

        assert!(manager.permission_broker().is_some());
        assert_eq!(
            manager.process_id(),
            Some(cx.read(|app| app.current_process_id()))
        );
    }

    #[cfg(feature = "media")]
    #[test]
    fn media_key_binding_routes_playback_keys_to_video_controller() {
        let cx = TestAppContext::single();
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));

        cx.read(|app| {
            MediaKeyBindingBuilder::new()
                .video(controller.clone())
                .install(app)
        });

        cx.simulate_media_key_event(MediaKeyEvent::Pause);
        cx.simulate_media_key_event(MediaKeyEvent::Stop);

        let events = controller.drain_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, VideoEvent::Paused))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, VideoEvent::Stopped))
        );
    }

    #[cfg(feature = "media")]
    #[test]
    fn media_key_binding_routes_track_and_unhandled_callbacks() {
        let cx = TestAppContext::single();
        let next_count = Rc::new(std::cell::Cell::new(0));
        let previous_count = Rc::new(std::cell::Cell::new(0));
        let unhandled_count = Rc::new(std::cell::Cell::new(0));

        cx.read(|app| {
            MediaKeyBindingBuilder::new()
                .on_next_track({
                    let next_count = next_count.clone();
                    move |_| next_count.set(next_count.get() + 1)
                })
                .on_previous_track({
                    let previous_count = previous_count.clone();
                    move |_| previous_count.set(previous_count.get() + 1)
                })
                .on_unhandled({
                    let unhandled_count = unhandled_count.clone();
                    move |_, _| unhandled_count.set(unhandled_count.get() + 1)
                })
                .install(app)
        });

        cx.simulate_media_key_event(MediaKeyEvent::NextTrack);
        cx.simulate_media_key_event(MediaKeyEvent::PreviousTrack);
        cx.simulate_media_key_event(MediaKeyEvent::Stop);

        assert_eq!(next_count.get(), 1);
        assert_eq!(previous_count.get(), 1);
        assert_eq!(unhandled_count.get(), 1);
    }
}
