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
kael = { version = "0.4", features = ["webview"] }
```

The core exports the `HttpClient` interface without selecting a transport.
Enable `http-client` for Kael's Reqwest adapter, supply an app-owned transport,
or use `kael_ui`'s default `http` feature, which enables the adapter for remote
assets:

```toml
[dependencies]
kael = { version = "0.4", features = ["http-client"] }
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
| Basic notifications | `kael` | Portable async delivery is in `kael_notifications`; browser delivery is permission gated |
| Storage and cache | `kael_storage`, `kael_cache` | App-owned data and bounded caches |
| Credentials | `kael_secrets` | Keychain, Credential Manager, or Secret Service |
| Documents, Office, and PDF | `kael_document`, `kael_office`, `kael_pdf` | Lifecycle, autosave, portable OOXML packages, and PDF byte operations |
| Sharing | `kael_share` or core `share` feature | Native share services or the user-activated browser Web Share picker |
| Networking | core `http-client`, `kael_http_client`, `kael_net` | Bounded HTTP plus one native/browser WebSocket client; SSE remains descriptor-only |
| Diagnostics | `kael_diagnostics` | Bounded logs, metrics, reports, crash helper |
| Updates | `kael_release` and core `auto-update` feature | Signed feeds and product-controlled installation |
| Media and audio | `kael-media`, `kael_audio`, engine crates | Native streams plus asynchronous bounded browser Web Audio; codecs and engines remain opt-in |
| Web compatibility | core `webview` feature | WebView2 on Windows, WebKitGTK on Linux |

The portable WebSocket client is documented in
[Realtime Networking](realtime-networking.md). Opening a socket requires an
explicit checked host policy; the core `AppRealtimeConnection` adapter applies
its `NetworkPolicy` again at the live side-effect boundary.

## Notifications

Basic OS notification delivery and action support live in `kael`. The
`kael_notifications` adds validated categories, immediate delivery,
native-only in-process interval scheduling, cancellation, event subscriptions,
and typed permission/backend errors. Use
`NotificationCenter::schedule_local_async` as the one-codebase call. Browser
delivery uses the Notification API after an asynchronous permission decision;
it does not fake durable scheduling, service-worker action buttons, push, named
sounds, or badge counts.

Packaged Windows applications must register an AppUserModelID in their installer
and call `set_windows_app_user_model_id` before delivering toasts. Push tokens,
calendar triggers, location triggers, and macOS text-input actions are not
implemented by Kael 0.4; product-specific push credentials and delegates remain
the application's responsibility.

## Sharing

`kael_share` validates text, URL, image, and file payloads before platform
handoff. Query `ShareSheet::platform_support()` before presenting destinations.
macOS supports the broadest outbound sheet. Windows and Linux currently provide
narrower mail/clipboard paths; Windows file/image DataTransferManager support
and share-receiver registration are not implemented. In browsers,
`ShareSheet::show_portable` uses `navigator.share` during a transient user
activation and supports bounded `ShareFile` bytes and images when
`navigator.canShare` accepts them. Browser policy, cancellation, unavailable
APIs, unsupported payloads, and lost activation are separate typed errors.

## Printing

`Window::print`, `Window::show_print_dialog`, and `Window::print_checked` now
execute native `PrintJob` content on every desktop backend. Fills, rounded fills,
strokes, lines, single-line and wrapped text, and retained images (including
fit, clipping, selected frame, and opacity) share a bounded portable renderer.
Landscape jobs expose rotated page/content dimensions to the render callback so
the same coordinates are not clipped when the output backend rotates the paper.

Windows shows the native common print dialog or silently targets the configured
default printer, then spools checked page rasters through the selected printer
DC with cleanup on every failure. Linux dialog printing uses the XDG desktop
Print portal and passes an exact PDF file descriptor; silent Linux printing uses
an absolute system `lp`/`lpr` client path. Accordingly, Linux printing remains a
partial runtime capability: dialog mode needs a portal backend, and silent mode
needs CUPS plus a default printer. Browser printing always shows browser-owned
UI because the web platform does not permit silent printer selection.

## Capture and media

Use `App::is_screen_capture_supported()` and query sources at runtime. Permission
approval and source availability can change after launch. A checked
`AppWindowCaptureRequest` is only a descriptor; Kael 0.4 does not expose a native
app-window screenshot backend through that type.

The portable `CaptureManager::with_default_backends()` API supports screen and
window capture on desktop and browser builds. In a browser, enumeration returns
a privacy-safe synthetic picker entry and `start` must run directly inside a
trusted click or key activation. The browser then chooses the exact display,
window, or tab asynchronously. A session first reports `Starting`, then
`Running` or `Error`; inspect `CaptureSession::last_error()` for picker denial,
lost activation, or setup failure. Browser frames are bounded RGBA8 buffers and
requested video audio is rejected explicitly—compose microphone input through
`kael_audio` when recording or calling.

Media engines are separate crates so applications that only need UI primitives
do not pull codecs and editing infrastructure into their dependency graph.
Browser live mixing, device enumeration, and permission-gated microphone
capture are documented in [Browser Audio](browser-audio.md). The capability
report marks microphone capture `RequiresInit` only when the `audio` feature is
compiled, and marks Kael's lightweight stereo spatial scene `Partial`; neither
status claims synchronous permission, HRTF, room processing, or non-default
browser speaker routing.

## WebView boundaries

Treat every WebView as an external-content boundary:

- restrict navigation and new-window behavior;
- allow only required permissions;
- validate messages crossing the bridge;
- choose persistent or ephemeral storage deliberately;
- keep native app state outside browser storage where possible.

WebViews are native composition islands, not Kael scene primitives. Rectangular
content-mask bounds, translation, visibility, and inherited opacity are applied
to the native host. Scale, rotation, or skew hides the host because resizing a
native WebView would reflow the page instead of reproducing the GPU transform.
Native surfaces remain above Kael's GPU scene, so applications should place
modals and popovers outside a visible WebView region or hide the island while
presenting overlapping chrome.

Omitting `storage_key` creates an incognito/non-persistent profile. Supplying a
key creates stable isolated profiles on Windows and Linux and on macOS 14 or
newer; older macOS versions retain persistence but share the default data
store. Windows and Linux additionally namespace profiles by executable path so
unrelated Kael applications cannot share a profile key; moving the executable
starts a fresh profile. Use `native_permission_policy` for the browser engine's actual permission
boundary. `on_permission_request` remains a page-level JavaScript preflight for
app-owned browser APIs and is not a native security boundary.
Native coverage follows the engine. The detailed
`native_permission_request_policy` receives WebView2's requesting origin and
user-gesture state on Windows (frame identity remains unknown), the current
top-level origin as an explicitly labelled approximation on WebKitGTK, and the
requesting origin plus main/subframe identity for WKWebView camera and
microphone requests. Permission decisions are not persisted by Kael's WebView2
hook, so the application policy remains authoritative on every request. The
legacy `native_permission_policy` intentionally discards this context; use it
only for policies that are safe by permission kind alone.

Linux uses one maintained child-host path across X11, XWayland, and native
Wayland. The portable `webview` feature selects GTK4 + WebKitGTK 6: GTK owns the
top-level surface, Kael renders its retained scene through GSK, and WebKit views
are siblings in the same widget hierarchy. It is not a detached overlay.
Bounds, clipping, visibility, focus, scale changes, IME, touch, clipboard, file
drop, window lifetime, native pointer lock, and raw Wayland/X11 handles therefore
belong to the same production window. `webview-gtk4` and
`webview-wayland-gtk4` are compatibility aliases for Linux-only manifests.

Automatic selection follows `GDK_BACKEND` ordering when valid display sockets
exist, then prefers Wayland when both displays are available. Set
`KAEL_LINUX_BACKEND=x11` or `wayland` to override it. The old GTK3/WebKitGTK 4.1
host is not shipped; the deprecated `webview-legacy-gtk3` spelling redirects to
the same maintained GTK4/WebKitGTK 6 host.

When the `webview` feature is disabled, the capability report returns
`SupportLevel::Disabled` rather than claiming the OS backend is usable.
With a native host feature enabled, the report returns `SupportLevel::Partial`
because WebViews are rectangular native islands and cannot participate in
arbitrary retained-scene transforms. A raw Wayland or X11 build without the
GTK4 WebView host, and every headless backend, reports WebViews as unsupported.
This lets an application reject a hard requirement before it opens a window.
The implementation contract and acceptance gates are documented in
[Linux WebView hosting](linux-webviews.md).

### Operation-level WebView capabilities

`CapabilityReport::current()` answers whether a WebView host is available.
`WebViewCapabilityReport::current()` is the release-grade operation matrix for
the selected backend. It reports each of composition, focus, navigation and
policy, load state, history, reload/stop, zoom, find, print, downloads, IPC,
custom protocols, developer tools, cookies, request headers, profile isolation,
user agent, JavaScript, native permission policy, and drag/drop as `Full`,
`Partial`, `Unsupported`, or `Disabled`, with an exact limitation note.

Important current boundaries are explicit:

- macOS, Windows, and Linux X11/XWayland preserve a live document when
  declarative focus changes; focus no longer recreates the host or loses
  history/profile state.
- Windows and Linux provide isolated named profiles. WKWebView named profile
  isolation requires macOS 14 or newer; older macOS versions share the
  persistent default data store.
- macOS WKWebView, Windows WebView2, and both Linux WebKitGTK hosts report
  `CustomProtocols` as `Full`. Registered app-owned routes preserve status,
  MIME type, response headers, and bounded body bytes for main documents and
  subresources. Browser iframes still report this operation as unsupported
  because web pages cannot register arbitrary URL schemes; use HTTP(S),
  `blob:`, `data:`, or inline HTML for browser-hosted islands.
- Browser iframe host navigation policy covers initial/declarative/controller
  loads, but browser security prevents the parent from synchronously vetoing an
  arbitrary cross-origin page's self-navigation, popup, or download.

Use `WebViewCapabilityReport::for_backend(...)` in release tooling to render a
deterministic cross-platform matrix without pretending the current machine is
evidence for every engine.

## Unsupported is an actionable result

Kael reports native push registration, geolocation, spellchecking, hardware
device discovery/I/O, file-promise drag sources, and app-window snapshot
backends as unsupported in 0.4. Applications can hide the workflow, provide an
owned integration, or keep a scoped WebView fallback. Do not treat the presence
of a request builder as backend support.
