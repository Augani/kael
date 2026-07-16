use crate::{
    Bounds, Pixels, SharedString,
    webview::{PlatformWebView, PlatformWebViewCommand},
};
use anyhow::Result;
use wry::{
    Rect, WebContext,
    dpi::{LogicalPosition, LogicalSize},
};

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

pub(crate) fn to_wry_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        position: LogicalPosition::new(bounds.origin.x.0 as f64, bounds.origin.y.0 as f64).into(),
        size: LogicalSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64).into(),
    }
}

pub(crate) fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

pub(crate) fn bridge_script(storage_key: Option<&SharedString>) -> String {
    let storage_key = storage_key
        .map(|storage_key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string_literal(storage_key.as_ref())
            )
        })
        .unwrap_or_default();

    format!(
        "(() => {{ {storage_key} if (!window.external) {{ window.external = {{}}; }} window.external.invoke = function(message) {{ const payload = typeof message === 'string' ? message : JSON.stringify(message); window.ipc.postMessage(payload); }}; if (!window.gpui) {{ window.gpui = {{}}; }} window.gpui.postMessage = function(message) {{ window.external.invoke(message); }}; }})();"
    )
}

pub(crate) fn css_script(css: &str) -> String {
    format!(
        "(() => {{ const mount = () => {{ if (!document.head) {{ return; }} const style = document.createElement('style'); style.setAttribute('data-gpui-webview-style', 'true'); style.textContent = {}; document.head.appendChild(style); }}; if (document.head) {{ mount(); }} else {{ document.addEventListener('DOMContentLoaded', mount, {{ once: true }}); }} }})();",
        json_string_literal(css)
    )
}

pub(crate) fn create_web_context(desired: &PlatformWebView) -> Result<Option<WebContext>> {
    desired
        .storage_key
        .as_ref()
        .map(webview_storage_dir)
        .transpose()
        .map(|directory| directory.map(|data_directory| WebContext::new(Some(data_directory))))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn webview_storage_dir(storage_key: &SharedString) -> Result<std::path::PathBuf> {
    use anyhow::Context as _;
    use std::{env, fs, path::PathBuf};

    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .context("XDG_DATA_HOME or HOME environment variable not set for webview storage")?;

    let directory = base
        .join(env!("CARGO_PKG_NAME"))
        .join("webview")
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

    let directory = base
        .join(env!("CARGO_PKG_NAME"))
        .join("webview")
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
