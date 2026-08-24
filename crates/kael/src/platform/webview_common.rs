#[cfg(all(any(), any(target_os = "linux", target_os = "freebsd")))]
use crate::webview::{WebViewPermissionDecision, WebViewPermissionKind};
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
use crate::{AsyncWindowContext, Bounds, Pixels, webview::PlatformWebView};
use crate::{SharedString, webview::PlatformWebViewCommand};
use anyhow::Result;
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
    },
};
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
use wry::{
    Rect, WebContext,
    dpi::{LogicalPosition, LogicalSize},
};
#[cfg(all(any(), any(target_os = "linux", target_os = "freebsd")))]
use wry_legacy as wry;

pub(crate) fn webview_command_id(command: &PlatformWebViewCommand) -> SharedString {
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

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn to_wry_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        position: LogicalPosition::new(bounds.origin.x.0 as f64, bounds.origin.y.0 as f64).into(),
        size: LogicalSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64).into(),
    }
}

pub(crate) fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

pub(crate) fn serialized_origin(url: &str) -> Option<SharedString> {
    let url = http_client::Url::parse(url).ok()?;
    let origin = url.origin().ascii_serialization();
    (origin != "null").then(|| origin.into())
}

/// Verify that the URI attached by the native WebView engine belongs to the
/// top-level origin Kael currently expects. When a document has an opaque
/// origin (for example `load_html`), the per-host nonce remains the security
/// boundary because there is no serializable origin to compare.
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn ipc_source_matches_top_level(source_uri: &str, expected: Option<&str>) -> bool {
    match expected {
        Some(expected) => {
            serialized_origin(source_uri).is_some_and(|source| source.as_ref() == expected)
        }
        None => true,
    }
}

/// Report a rejected IPC message at most once per WebView host so a hostile
/// child frame cannot turn rejection logging into an application-level DoS.
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn warn_rejected_ipc_once(reported: &AtomicBool, platform: &str) {
    if !reported.swap(true, Ordering::Relaxed) {
        log::warn!(
            "rejected untrusted {platform} WebView IPC message; further rejections are suppressed"
        );
    }
}

const IPC_NONCE_FIELD: &str = "__kaelIpcNonce";
const IPC_BODY_FIELD: &str = "body";

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn bridge_script(storage_key: Option<&SharedString>, nonce: &str) -> String {
    let storage_key = storage_key
        .map(|storage_key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string_literal(storage_key.as_ref())
            )
        })
        .unwrap_or_default();

    let nonce = json_string_literal(nonce);
    format!(
        "(() => {{ if (window.top !== window.self) return; {storage_key} const nonce = {nonce}; if (!window.external) {{ window.external = {{}}; }} window.external.invoke = function(message) {{ const body = typeof message === 'string' ? message : JSON.stringify(message); window.ipc.postMessage(JSON.stringify({{ {IPC_NONCE_FIELD}: nonce, {IPC_BODY_FIELD}: body }})); }}; if (!window.gpui) {{ window.gpui = {{}}; }} window.gpui.postMessage = function(message) {{ window.external.invoke(message); }}; }})();"
    )
}

/// Wrap an initialization script so Wry's Windows subframe injection cannot
/// accidentally run app-owned code outside the top-level document.
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn main_frame_script(script: &str) -> String {
    format!("(() => {{ if (window.top !== window.self) return; {script} }})();")
}

/// Authenticate and decode a message emitted by [`bridge_script`].
pub(crate) fn decode_bridge_message(body: &str, nonce: &str) -> Option<serde_json::Value> {
    let envelope: serde_json::Value = serde_json::from_str(body).ok()?;
    if envelope.get(IPC_NONCE_FIELD)?.as_str()? != nonce {
        return None;
    }
    let body = envelope.get(IPC_BODY_FIELD)?.as_str()?;
    Some(serde_json::from_str(body).unwrap_or_else(|_| serde_json::Value::String(body.to_owned())))
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn css_script(css: &str) -> String {
    format!(
        "(() => {{ const mount = () => {{ if (!document.head) {{ return; }} const style = document.createElement('style'); style.setAttribute('data-gpui-webview-style', 'true'); style.textContent = {}; document.head.appendChild(style); }}; if (document.head) {{ mount(); }} else {{ document.addEventListener('DOMContentLoaded', mount, {{ once: true }}); }} }})();",
        json_string_literal(css)
    )
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn create_web_context(desired: &PlatformWebView) -> Result<Option<WebContext>> {
    desired
        .storage_key
        .as_ref()
        .map(webview_storage_dir)
        .transpose()
        .map(|directory| directory.map(|data_directory| WebContext::new(Some(data_directory))))
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
thread_local! {
    static WRY_PROTOCOL_CONTEXTS: RefCell<HashMap<u64, AsyncWindowContext>> =
        RefCell::new(HashMap::new());
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
static NEXT_WRY_PROTOCOL_CONTEXT: AtomicU64 = AtomicU64::new(1);

/// Main-thread context registration used by Wry's custom-protocol callbacks.
///
/// Wry requires protocol closures to be `Send + Sync` even though WebKitGTK
/// and WebView2 invoke them on the WebView UI thread. Capturing Kael's
/// `AsyncWindowContext` directly would therefore be unsound. The closure
/// captures only this numeric token and resolves the non-Send context from
/// thread-local storage on the actual UI thread. An engine regression that
/// invokes it elsewhere fails closed with a bounded 500 response.
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) struct WryCustomProtocolRegistration {
    token: u64,
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
impl WryCustomProtocolRegistration {
    pub(crate) fn new(desired: &PlatformWebView) -> Self {
        let token = NEXT_WRY_PROTOCOL_CONTEXT.fetch_add(1, AtomicOrdering::Relaxed);
        WRY_PROTOCOL_CONTEXTS.with(|contexts| {
            contexts
                .borrow_mut()
                .insert(token, desired.async_window.clone());
        });
        Self { token }
    }

    pub(crate) fn update(&self, desired: &PlatformWebView) {
        WRY_PROTOCOL_CONTEXTS.with(|contexts| {
            contexts
                .borrow_mut()
                .insert(self.token, desired.async_window.clone());
        });
    }
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
impl Drop for WryCustomProtocolRegistration {
    fn drop(&mut self) {
        WRY_PROTOCOL_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().remove(&self.token);
        });
    }
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn configure_wry_custom_protocols<'a>(
    mut builder: wry::WebViewBuilder<'a>,
    desired: &PlatformWebView,
    registration: &WryCustomProtocolRegistration,
) -> wry::WebViewBuilder<'a> {
    for scheme in &desired.custom_protocol_schemes {
        let token = registration.token;
        let wrong_thread_reported = Arc::new(AtomicBool::new(false));
        builder = builder.with_custom_protocol(scheme.to_string(), move |_, request| {
            WRY_PROTOCOL_CONTEXTS.with(|contexts| {
                // Release the registry borrow before entering application code. A
                // protocol handler may synchronously rerender or close this host,
                // which updates/removes the registration on the same UI thread.
                let context = { contexts.borrow().get(&token).cloned() };
                let Some(mut context) = context else {
                    if !wrong_thread_reported.swap(true, Ordering::Relaxed) {
                        log::error!(
                            "Wry invoked a Kael custom protocol outside its registered WebView UI thread"
                        );
                    }
                    return wry_protocol_error(500, "Internal Server Error");
                };
                match context.update(|_, cx| {
                    cx.handle_custom_protocol_url(request.uri().to_string())
                }) {
                    Ok(Ok(Some(response))) => wry_protocol_response(response),
                    Ok(Ok(None)) => wry_protocol_error(404, "Not Found"),
                    Ok(Err(error)) | Err(error) => {
                        log::warn!("serving Wry custom protocol failed: {error:#}");
                        wry_protocol_error(500, "Internal Server Error")
                    }
                }
            })
        });
    }
    builder
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
fn wry_protocol_response(
    response: crate::CustomProtocolResponse,
) -> wry::http::Response<Cow<'static, [u8]>> {
    let mut builder = wry::http::Response::builder()
        .status(response.status)
        .header(wry::http::header::CONTENT_TYPE, response.mime_type);
    for (name, value) in response.headers {
        if !name.eq_ignore_ascii_case("content-type") {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Cow::Owned(response.body))
        .unwrap_or_else(|error| {
            log::warn!("building Wry custom protocol response failed: {error}");
            wry_protocol_error(500, "Internal Server Error")
        })
}

#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
fn wry_protocol_error(
    status: u16,
    message: &'static str,
) -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .status(status)
        .header(wry::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(wry::http::header::CACHE_CONTROL, "no-store")
        .body(Cow::Borrowed(message.as_bytes()))
        .expect("static Wry protocol error response is valid")
}

fn webview_storage_app_namespace(executable: &std::path::Path) -> String {
    format!(
        "{:016x}",
        seahash::hash(executable.to_string_lossy().as_bytes())
    )
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn permission_kind_from_wry(kind: wry::PermissionKind) -> WebViewPermissionKind {
    match kind {
        wry::PermissionKind::Microphone => WebViewPermissionKind::Microphone,
        wry::PermissionKind::Camera => WebViewPermissionKind::Camera,
        wry::PermissionKind::Geolocation => WebViewPermissionKind::Geolocation,
        wry::PermissionKind::Notifications => WebViewPermissionKind::Notifications,
        wry::PermissionKind::ClipboardRead => WebViewPermissionKind::ClipboardRead,
        wry::PermissionKind::DisplayCapture => WebViewPermissionKind::DisplayCapture,
        wry::PermissionKind::Midi => WebViewPermissionKind::Midi,
        wry::PermissionKind::Sensors => WebViewPermissionKind::Sensors,
        wry::PermissionKind::MediaKeySystemAccess => WebViewPermissionKind::MediaKeySystemAccess,
        wry::PermissionKind::LocalFonts => WebViewPermissionKind::LocalFonts,
        wry::PermissionKind::WindowManagement => WebViewPermissionKind::WindowManagement,
        wry::PermissionKind::PointerLock => WebViewPermissionKind::PointerLock,
        wry::PermissionKind::AutomaticDownloads => WebViewPermissionKind::AutomaticDownloads,
        wry::PermissionKind::FileSystemAccess => WebViewPermissionKind::FileSystemAccess,
        wry::PermissionKind::Autoplay => WebViewPermissionKind::Autoplay,
        _ => WebViewPermissionKind::Other,
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[cfg(any(
    all(feature = "webview", target_os = "windows"),
    all(any(), any(target_os = "linux", target_os = "freebsd"))
))]
pub(crate) fn permission_response_to_wry(
    decision: WebViewPermissionDecision,
) -> wry::PermissionResponse {
    match decision {
        WebViewPermissionDecision::Allow => wry::PermissionResponse::Allow,
        WebViewPermissionDecision::Deny => wry::PermissionResponse::Deny,
        WebViewPermissionDecision::Default => wry::PermissionResponse::Default,
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) fn webview_storage_dir(storage_key: &SharedString) -> Result<std::path::PathBuf> {
    use anyhow::Context as _;
    use std::{env, fs, path::PathBuf};

    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .context("XDG_DATA_HOME or HOME environment variable not set for webview storage")?;
    let executable =
        env::current_exe().context("resolving application executable for webview storage")?;

    let directory = base
        .join(env!("CARGO_PKG_NAME"))
        .join("webview")
        // CARGO_PKG_NAME identifies this library (`kael`), not the embedding
        // application. Without an application namespace, unrelated Kael apps
        // choosing a common profile key such as `auth` share cookies and local
        // storage. Do not migrate the legacy shared directory: its contents
        // cannot be attributed safely to any one application.
        .join(webview_storage_app_namespace(&executable))
        .join(format!(
            "{:016x}",
            seahash::hash(storage_key.as_ref().as_bytes())
        ));
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "creating Linux webview storage directory {}",
            directory.display()
        )
    })?;
    Ok(directory)
}

#[cfg(target_os = "windows")]
fn webview_storage_dir(storage_key: &SharedString) -> Result<std::path::PathBuf> {
    use anyhow::Context as _;
    use std::{env, fs, path::PathBuf};

    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("APPDATA").map(PathBuf::from))
        .context("LOCALAPPDATA or APPDATA environment variable not set for webview storage")?;
    let executable =
        env::current_exe().context("resolving application executable for webview storage")?;

    let directory = base
        .join(env!("CARGO_PKG_NAME"))
        .join("webview")
        .join(webview_storage_app_namespace(&executable))
        .join(format!(
            "{:016x}",
            seahash::hash(storage_key.as_ref().as_bytes())
        ));
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "creating Windows webview storage directory {}",
            directory.display()
        )
    })?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    #[cfg(any(
        all(feature = "webview", target_os = "windows"),
        all(any(), any(target_os = "linux", target_os = "freebsd"))
    ))]
    use super::{
        bridge_script, ipc_source_matches_top_level, main_frame_script, wry_protocol_error,
        wry_protocol_response,
    };
    use super::{decode_bridge_message, serialized_origin, webview_storage_app_namespace};
    use std::path::Path;

    #[test]
    fn persistent_storage_namespace_is_stable_and_application_specific() {
        let first = webview_storage_app_namespace(Path::new("/opt/apps/first/bin/app"));
        assert_eq!(
            first,
            webview_storage_app_namespace(Path::new("/opt/apps/first/bin/app"))
        );
        assert_ne!(
            first,
            webview_storage_app_namespace(Path::new("/opt/apps/second/bin/app"))
        );
    }

    #[test]
    fn bridge_messages_require_the_per_host_nonce() {
        let body = serde_json::json!({
            "__kaelIpcNonce": "expected",
            "body": r#"{"kind":"ready"}"#,
        })
        .to_string();
        assert_eq!(
            decode_bridge_message(&body, "expected"),
            Some(serde_json::json!({ "kind": "ready" }))
        );
        assert_eq!(decode_bridge_message(&body, "wrong"), None);
        assert_eq!(decode_bridge_message("not-json", "expected"), None);
    }

    #[test]
    #[cfg(any(
        all(feature = "webview", target_os = "windows"),
        all(any(), any(target_os = "linux", target_os = "freebsd"))
    ))]
    fn initialization_scripts_are_main_frame_only() {
        let bridge = bridge_script(None, "nonce");
        let custom = main_frame_script("window.custom = true;");
        assert!(bridge.contains("window.top !== window.self"));
        assert!(bridge.contains("__kaelIpcNonce"));
        assert!(custom.contains("window.top !== window.self"));
    }

    #[test]
    fn serialized_origins_drop_paths_and_default_ports() {
        assert_eq!(
            serialized_origin("https://example.test:443/path?q=1")
                .as_ref()
                .map(|origin| origin.as_ref()),
            Some("https://example.test")
        );
        assert_eq!(serialized_origin("data:text/plain,hello"), None);
    }

    #[test]
    #[cfg(any(
        all(feature = "webview", target_os = "windows"),
        all(any(), any(target_os = "linux", target_os = "freebsd"))
    ))]
    fn ipc_sources_are_bound_to_the_expected_top_level_origin() {
        assert!(ipc_source_matches_top_level(
            "https://example.test/frame",
            Some("https://example.test")
        ));
        assert!(!ipc_source_matches_top_level(
            "https://attacker.test/frame",
            Some("https://example.test")
        ));
        assert!(!ipc_source_matches_top_level(
            "data:text/plain,opaque",
            Some("https://example.test")
        ));
        assert!(ipc_source_matches_top_level("data:text/plain,opaque", None));
    }

    #[cfg(any(
        all(feature = "webview", target_os = "windows"),
        all(any(), any(target_os = "linux", target_os = "freebsd"))
    ))]
    #[test]
    fn wry_custom_protocol_response_preserves_checked_metadata_and_bytes() {
        let response = wry_protocol_response(crate::CustomProtocolResponse {
            status: 201,
            mime_type: "application/octet-stream".to_string(),
            headers: vec![
                ("X-Kael-Probe".to_string(), "served".to_string()),
                ("Content-Type".to_string(), "text/plain".to_string()),
            ],
            body: vec![0, 1, 2, 255],
        });
        assert_eq!(response.status(), 201);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/octet-stream"
        );
        assert_eq!(response.headers().get("x-kael-probe").unwrap(), "served");
        assert_eq!(response.body().as_ref(), &[0, 1, 2, 255]);
    }

    #[cfg(any(
        all(feature = "webview", target_os = "windows"),
        all(any(), any(target_os = "linux", target_os = "freebsd"))
    ))]
    #[test]
    fn wry_custom_protocol_errors_are_uncacheable_and_bounded() {
        let response = wry_protocol_error(404, "Not Found");
        assert_eq!(response.status(), 404);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(response.body().as_ref(), b"Not Found");
    }
}
