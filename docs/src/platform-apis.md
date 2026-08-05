# Platform APIs

Kael exposes native desktop services through the core runtime and focused
support crates. Availability varies by operating system, installed desktop
services, permissions, packaging, and compile-time features.

## Capability checks

Use `CapabilityReport::current()` before making a platform integration a hard
requirement:

```rust
use kael::{CapabilityCheck, CapabilityReport, PlatformFeature};

let report = CapabilityReport::current();
let readiness = CapabilityCheck::new()
    .require(PlatformFeature::GpuRendering)
    .prefer_available(PlatformFeature::GlobalHotkeys)
    .prefer_available(PlatformFeature::StatusBarItem)
    .evaluate(&report);

if let Some(reason) = readiness.required_failure_summary() {
    return Err(reason.into());
}
```

Use `require` for features that must be fully supported and
`require_available` only when the application has implemented the setup and
fallback paths for `Partial` or `RequiresInit`.

## Compile-time features

The default `kael` build enables the native window backends but keeps WebView
support off. Applications that embed web content can opt into Wry and the
platform WebView dependencies:

```toml
[dependencies]
kael = { version = "0.3", features = ["webview"] }
```

The core exports the `HttpClient` interface without selecting a transport.
Enable `http-client` for Kael's Reqwest adapter, supply an app-owned transport,
or use `kael_ui`'s default `http` feature, which enables the adapter for remote
assets:

```toml
[dependencies]
kael = { version = "0.3", features = ["http-client"] }
```

Use `wayland` instead of, or alongside, `x11` as required. Other product
services are opt-in through core features or their focused crates. Enable
`auto-update` for Kael's signed-feed, checked-download, and platform-installer
pipeline, and `lottie` for native Lottie/dotLottie playback. Leaving these
features off keeps their implementation dependencies out of the application.

## Service overview

| Area | API or crate | Notes |
| --- | --- | --- |
| Windows, displays, input, clipboard, menus | `kael` | Native per-OS backends |
| Accessibility | `kael` + AccessKit bridges | Semantics still require correct app markup |
| Global hotkeys and tray | `kael` | Linux support depends on X11 or desktop portals/SNI |
| Basic notifications | `kael` | Rich scheduling is in `kael_notifications` |
| Storage and cache | `kael_storage`, `kael_cache` | App-owned data and bounded caches |
| Credentials | `kael_secrets` | Keychain, Credential Manager, or Secret Service |
| Documents and PDF | `kael_document`, `kael_pdf` | Lifecycle, autosave, versions, and PDF operations |
| Sharing | `kael_share` or core `share` feature | Destination coverage differs by OS |
| Networking | core `http-client`, `kael_http_client`, `kael_net` | Optional Reqwest transport plus higher-level network policy/state |
| Diagnostics | `kael_diagnostics` | Bounded logs, metrics, reports, crash helper |
| Updates | `kael_release` and core `auto-update` feature | Signed feeds and product-controlled installation |
| Media | `kael-media`, `kael_audio`, engine crates | Opt-in due to codecs and platform dependencies |
| Web compatibility | core `webview` feature | WebView2 on Windows, WebKitGTK on Linux |

## Notifications

Basic OS notification delivery and action support live in `kael`. The
`kael_notifications` crate adds validated categories, in-process immediate/time
interval scheduling, cancellation, event subscriptions, badge state, and
platform delivery.

Packaged Windows applications must register an AppUserModelID in their installer
and call `set_windows_app_user_model_id` before delivering toasts. Push tokens,
calendar triggers, location triggers, and macOS text-input actions are not
implemented by Kael 0.3; product-specific push credentials and delegates remain
the application's responsibility.

## Sharing

`kael_share` validates text, URL, image, and file payloads before platform
handoff. Query `ShareSheet::platform_support()` before presenting destinations.
macOS supports the broadest outbound sheet. Windows and Linux currently provide
narrower mail/clipboard paths; Windows file/image DataTransferManager support
and share-receiver registration are not implemented.

## Capture and media

Use `App::is_screen_capture_supported()` and query sources at runtime. Permission
approval and source availability can change after launch. A checked
`AppWindowCaptureRequest` is only a descriptor; Kael 0.3 does not expose a native
app-window screenshot backend through that type.

Media engines are separate crates so applications that only need UI primitives
do not pull codecs and editing infrastructure into their dependency graph.

## WebView boundaries

Treat every WebView as an external-content boundary:

- restrict navigation and new-window behavior;
- allow only required permissions;
- validate messages crossing the bridge;
- choose persistent or ephemeral storage deliberately;
- keep native app state outside browser storage where possible.

When the `webview` feature is disabled, the capability report returns
`SupportLevel::Disabled` rather than claiming the OS backend is usable.

## Unsupported is an actionable result

Kael reports native push registration, geolocation, spellchecking, hardware
device discovery/I/O, file-promise drag sources, and app-window snapshot
backends as unsupported in 0.3. Applications can hide the workflow, provide an
owned integration, or keep a scoped WebView fallback. Do not treat the presence
of a request builder as backend support.
