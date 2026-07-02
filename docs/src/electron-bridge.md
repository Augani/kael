# Bridging the Electron Gap

Kael is a native-first alternative to Electron, not a drop-in browser runtime.
That distinction matters. Electron gives every app Chromium, Node.js, the DOM,
CSS, browser media, WebGL, WebCodecs, WebRTC, service workers, and the npm UI
ecosystem by default. Kael starts from the opposite end: one Rust application,
native windows, a GPU-rendered retained UI tree, and explicit platform APIs.

The replacement bar is therefore not "copy Electron". The bar is:

1. Make common desktop-app workflows easier than Electron.
2. Provide native high-performance equivalents for the web APIs app builders
   reach for most often.
3. Offer clear escape hatches when the whole web platform is the correct tool.
4. Document the current capability level honestly so developers and AI agents
   pick the right primitive on the first try.

## The builder ladder

When an app needs more control, builders should move down this ladder:

| Need | Kael primitive | Current status |
| --- | --- | --- |
| Standard app UI | `kael_ui` components | Broad coverage; best starting point |
| Custom visual design | styled `div()`, theme tokens, custom variants | Good coverage; expand recipes |
| Custom behavior with custom markup | headless controllers, semantic accessibility recipes | Available for common patterns; focus/a11y recipes now cover common custom controls |
| Custom drawing | canvas, paths, images, SVG, Lottie | Useful today; missing some browser canvas parity |
| Web-standard surface | `webview(id, url)` | Available; should be documented as an intentional escape hatch |
| Low-level GPU effects | render targets, custom shaders, render graph | Design exists; public API is still roadmap work |
| OS integration or isolated workload | platform APIs, worker processes, extensions | Good base; capability varies by OS |

This ladder is the core answer to "can I design any app?" Kael should not force
every problem through one abstraction. It should make the right abstraction
obvious.

## Media is the first bridge to build

Electron inherits the browser media element. A developer can create a video
player by setting a URL on a `<video>` element and controlling it with
JavaScript:

```js
video.src = url
await video.play()
video.currentTime = 42
video.playbackRate = 1.5
```

Kael currently has useful media primitives, but not this level of convenience.
`kael-media` can load `MediaSource::Url`, `File`, `Bytes`, and `Reader`, and
decode video through FFmpeg. The core `video(source)` element can display
frames. `VideoController` now provides the browser-shaped control layer:
metadata loading, play/pause/stop/seek, volume/mute, loop, audio playback-rate
control, ready-state changes, buffered ranges, WebVTT/SRT text tracks,
snapshots, and events.
`kael_ui::VideoPlayer::source(...)` now wires that controller into the existing
player chrome, renders selected text-track cues as captions, and renders the
core `video(source)` element behind the controls. `VideoPlayer::url(...)`
defaults to `VideoPlayerRoute::Auto`, so ordinary URL/file media uses the
native element while HLS/DASH-style manifest sources are routed through a
WebView-hosted browser `<video controls>` fallback. The built-in controls
expose loaded caption/subtitle tracks through a captions menu. The remaining
gap is moving decode, buffering, and rendering onto stronger media backends.

The implemented control layer looks like this:

```rust
use kael::{
    can_play_video_type, recommended_video_playback_route,
    recommended_video_playback_route_for_type, webview, webview_video_player_url,
    video_capability_report, MediaSourceBuilder, VideoCanPlay, VideoController, VideoEvent,
    VideoPlaybackPlanBuilder, VideoPlaybackPlanTarget, VideoPlaybackRoute, WebViewVideoOptions,
};
use std::time::Duration;

let capabilities = video_capability_report();
println!("hardware decode: {:?}", capabilities.hardware_decode);

let plan = VideoPlaybackPlanBuilder::url(video_url.clone())
    .content_type(content_type_header)
    .webview_options(WebViewVideoOptions::default().controls(true))
    .build_checked()?;

match plan.target() {
    VideoPlaybackPlanTarget::Native => {
        let video = plan.controller();
        video.load_metadata()?;
        video.play()?;
    }
    VideoPlaybackPlanTarget::WebViewFallback { page_url, element_id, .. } => {
        return webview(element_id.clone(), page_url.clone()).size_full().into_any_element();
    }
}

let support = can_play_video_type("video/mp4; codecs=\"avc1.42E01E\"");
if support < VideoCanPlay::Maybe {
    // Pick a WebView island or ask the user for a different source.
}

if matches!(
    recommended_video_playback_route(&MediaSourceBuilder::url(video_url.clone()).build_checked()?),
    VideoPlaybackRoute::WebViewRecommended { .. }
) {
    let source = MediaSourceBuilder::url(video_url.clone()).build_checked()?;
    let options = WebViewVideoOptions::default()
        .autoplay(true)
        .muted(true)
        .checked()?;
    let page = webview_video_player_url(
        &source,
        &options,
    )
    .expect("URL-backed media can be wrapped for WebView fallback");
    return webview("streaming-video", page).size_full().into_any_element();
}

let route_from_header = recommended_video_playback_route_for_type(content_type_header);
if route_from_header.should_use_webview() {
    // Extensionless HLS/DASH CDN URLs should follow the browser fallback too.
}

let video = MediaSourceBuilder::url(video_url)
    .controller_checked()?
    .volume(0.8)
    .muted(false)
    .playback_rate(1.0)
    .looping(false)
    .webvtt_text_track("en", "English", Some("en"), webvtt_source)
    .selected_text_track("en");

video.load_metadata()?;
video.play()?;
video.fast_seek(Duration::from_secs(42))?;
video.set_url(next_video_url);

for event in video.drain_events() {
    match event {
        VideoEvent::LoadedMetadata {
            duration,
            width,
            height,
        } => {
            println!("loaded {duration:?} at {width}x{height}");
        }
        VideoEvent::TimeUpdate { current_time } => {
            println!("time {current_time:?}");
        }
        VideoEvent::Progress { buffered_ranges } => {
            println!("buffered: {buffered_ranges:?}");
        }
        VideoEvent::CanPlay => {
            println!("ready to play");
        }
        VideoEvent::Error(error) => eprintln!("{error}"),
        _ => {}
    }
}

println!("state: {:?}", video.playback_state());
println!("time: {:?}", video.current_time());
println!("time seconds: {:?}", video.current_time_secs());
println!("duration: {:?}", video.duration());
println!("duration seconds: {:?}", video.duration_secs());
println!("ready: {:?}", video.ready_state());
println!("buffered: {:?}", video.buffered_ranges());
println!("muted: {:?}", video.is_muted());
println!("rate: {:?}", video.rate());

let snapshot = video.snapshot();
println!("snapshot cues: {:?}", snapshot.active_text_cues);
```

For web-familiar naming, `current_time_secs()`, `set_current_time_secs(...)`,
`fast_seek_secs(...)`, `duration_secs()`, `paused()`, `set_position(...)`,
`muted_state()`, `rate()`, and `looping_enabled()` are aliases around the
canonical Rust getters.
For source replacement, `set_source(...)`, `set_url(...)`, `set_file(...)`,
`set_bytes(...)`, and `set_reader(...)` reset media-derived state while
preserving volume, mute, playback-rate, loop, and text-track configuration.
Use `recommended_video_playback_route(...)`,
`recommended_video_playback_route_for_type(...)`, or
`VideoController::recommended_route()` before constructing advanced media UIs:
direct files/URLs default to native playback, while HLS (`.m3u8`) and DASH
(`.mpd`) manifests or adaptive streaming MIME types recommend an explicit
WebView island until native streaming backends land. The MIME helper is useful
for extensionless CDN URLs where the `Content-Type` header is the only reliable
signal.
Prefer `VideoPlaybackPlanBuilder` for generated URL players: it validates the
source, optional MIME type, and `WebViewVideoOptions`, then returns a single
target (`Native` or `WebViewFallback`) plus `can_play`, route, controller, and
fallback page/id accessors.
Use `can_play_video_type(...)`, `can_play_video_source(...)`, or
`VideoController::can_play_source()` when you need a browser-style support
confidence (`No`, `Maybe`, or `Probably`) before showing a native player.
Use `webview_video_player_url(...)` with `WebViewVideoOptions` to build a
browser `<video>` page for URL/file sources that should be routed through
WebView. The fallback accepts common HTML-video attributes such as poster,
preload, crossorigin, controlslist, disabled picture-in-picture, initial
current time, object-fit, and WebVTT `<track>` tags. The fallback page posts
browser media events back through the WebView bridge, and `VideoPlayer` maps
loaded metadata, readiness, progress, play/pause, time, seek, volume, rate,
text-track selection, active cue changes, browser fullscreen, picture-in-picture,
ended, and error messages into the same callback surface as native playback.
Drive custom fallback chrome with
`VideoController::dispatch_webview_command(window, WebViewVideoCommand::...)`:
commands cover play/pause/toggle/stop, exact and fast seek, volume, mute,
playback rate, loop, text-track selection/disablement, browser fullscreen,
picture-in-picture, and snapshot requests.
Use `video_capability_report()` when an app or agent needs an honest feature
matrix: source types, controller events, source replacement, can-play checks,
route recommendation, WebView fallback, text tracks, fast seek, playback rate,
fullscreen, adaptive streaming, hardware decode, and native stream selection.

The high-level player API is:

```rust
use kael::{ObjectFit, WebViewVideoCommand, WebViewVideoCrossOrigin, WebViewVideoOptions};
use kael_ui::prelude::*;
use std::time::Duration;

VideoPlayer::url(video_url, cx)
    .object_fit(ObjectFit::Contain)
    .playback_route(VideoPlayerRoute::Auto)
    .content_type(content_type_header)
    .preload(VideoPreload::Metadata)
    .controls(true)
    .volume(0.8)
    .muted(false)
    .playback_rate(1.0)
    .looping(false)
    .start_at(Duration::from_secs(0))
    .poster(poster_url)
    .webview_options(
        WebViewVideoOptions::default()
            .preload(kael::WebViewVideoPreload::Metadata)
            .cross_origin(WebViewVideoCrossOrigin::Anonymous)
            .controls_list(["nodownload"])
            .disable_picture_in_picture(true),
    )
    .show_captions(true)
    .webvtt_text_track("en", "English", Some("en"), webvtt_source)
    .select_text_track("en")
    .caption_style(
        VideoCaptionStyle::default()
            .background(kael::black().opacity(0.82))
            .font_size(px(16.0)),
    )
    .on_loaded_metadata(|duration, width, height, _window, _cx| {
        println!("loaded {duration:?} at {width}x{height}");
    })
    .on_can_play(|_window, _cx| {
        println!("ready to play");
    })
    .on_progress(|buffered_ranges, _window, _cx| {
        println!("buffered: {buffered_ranges:?}");
    })
    .on_time_update(|current_time, _window, _cx| {
        println!("time {current_time:?}");
    })
    .on_seeked(|current_time, _window, _cx| {
        println!("seeked to {current_time:?}");
    })
    .on_rate_change(|rate, _window, _cx| {
        println!("rate {rate}x");
    })
    .on_cue_change(|cues, _window, _cx| {
        println!("active cues: {}", cues.len());
    })
    .on_error(|error, _window, _cx| eprintln!("{error}"))
```

Use `VideoPlayer::file(...)`, `VideoPlayer::bytes(...)`,
`VideoPlayer::reader(...)`, or `VideoPlayer::source(MediaSource::...)` when the
source is not a URL. Auto routing is the default; use `.native_playback()` to
force Kael's native element, `.webview_fallback()` to force a browser `<video>`
island for URL/file sources, or `.webview_options(...)` to tune the fallback
page. Use `.content_type(...)` when an extensionless URL is known to be HLS,
DASH, or another adaptive streaming type from a response header. The high-level
`.poster(...)`, `.preload(...)`, `.start_at(...)`, and `.webvtt_text_track(...)`
builders are mirrored into the fallback options.
When Auto selects WebView, `.on_loaded_metadata(...)`, `.on_can_play(...)`,
`.on_progress(...)`, `.on_playing(...)`, `.on_paused(...)`,
`.on_time_update(...)`, `.on_seeked(...)`, `.on_volume_changed(...)`,
`.on_rate_change(...)`, `.on_ended(...)`, `.on_error(...)`, and `.on_event(...)`
still receive browser video events.

When an app needs to drive the browser fallback directly, use the same WebView
command channel Kael exposes for other embedded web surfaces:

```rust
let controller = player.controller().expect("source-backed player");
controller.dispatch_webview_command(
    window,
    WebViewVideoCommand::FastSeek(Duration::from_secs(90)),
)?;
controller.dispatch_webview_command(window, WebViewVideoCommand::SetPlaybackRate(1.5))?;
```

Parsed tracks can still be configured directly when needed:

```rust
let player = VideoPlayer::url(video_url, cx)
    .text_track(custom_track)
    .select_text_track("en");
```

For source-backed players, `.on_play(...)`, `.on_pause(...)`, `.on_seek(...)`,
`.on_volume_change(...)`, `.on_playback_speed_change(...)`,
`.on_source_changed(...)`, `.on_loaded_metadata(...)`,
`.on_ready_state_change(...)`, `.on_can_play(...)`, `.on_can_play_through(...)`,
`.on_waiting(...)`, `.on_progress(...)`, `.on_playing(...)`, `.on_paused(...)`,
`.on_stopped(...)`, `.on_time_update(...)`, `.on_seeked(...)`,
`.on_volume_changed(...)`, `.on_rate_change(...)`, `.on_loop_change(...)`,
`.on_text_track_added(...)`, `.on_text_track_changed(...)`, `.on_cue_change(...)`,
`.on_ended(...)`, and `.on_error(...)` are additive user hooks. Source-backed configuration methods such as `.controls(...)`,
`.autoplay()`, `.preload(...)`, `.volume(...)`, `.muted(...)`,
`.playback_rate(...)`, `.looping(...)`, `.start_at(...)`, `.srt_text_track(...)`,
`.webvtt_text_track(...)`, `.select_text_track(...)`, and
`.disable_text_track(...)` configure the internal `VideoController` or built-in
chrome directly. `VideoPreload::Metadata` and `VideoPreload::Auto` load metadata
up front; `Auto` will grow into deeper buffering as the backend gains accurate
streaming ranges. The internal controller still receives user commands first, so
custom analytics or app-state callbacks do not accidentally disconnect playback.
Keyboard controls and visible chrome use the same callbacks: space toggles
play/pause, arrows seek or adjust volume, `m` toggles mute, and `f` toggles real
window fullscreen.

and advanced apps should be able to split the rendering and controls:

```rust
let video = VideoController::url(video_url);

div()
    .child(VideoView::new(video.clone()).object_fit(ObjectFit::Cover))
    .child(VideoControls::new(video).overlay())
```

### Required media capabilities

Ship these as the public media contract before claiming Electron-like media:

- Source types: file path, URL, bytes, custom reader.
- Controls: play, pause, stop, seek, fast seek, rate, volume, mute, loop.
  Initial playback-rate support is implemented for audio output through the
  current software sink; pitch preservation and a stronger decoded-video clock
  remain backend work. `VideoController::fast_seek(...)` and
  `.fast_seek_secs(...)` are available for scrubbers; the current software
  backend uses the same stream-level seek path as exact seek until platform
  backends can prefer keyframe seeks.
- State: duration, current time, buffered ranges, ready state, dimensions,
  playback state, error state. Initial `VideoSnapshot::buffered_ranges` and
  `VideoReadyState` support is implemented; local file/bytes/reader sources
  report the full duration after metadata loads, while URL-backed streaming
  ranges remain unknown until native streaming backends can report them.
- Events: loaded metadata, can play, playing, pause, seeked, waiting, time
  update, source changed, ended, error. Initial `Progress`,
  `ReadyStateChange`, `CanPlay`, `CanPlayThrough`, and `Waiting` events are
  implemented.
- Rendering: object-fit, poster, placeholder, rounded clipping, overlays,
  real window fullscreen toggles, and fullscreen hooks. `VideoPlayer` uses
  `Window::toggle_fullscreen()` for its `f` keybinding and fullscreen button,
  then calls `.on_fullscreen(...)` with the platform-reported state.
- Audio/video sync: one controller owns the clock; UI reads state from it.
- Subtitles and tracks: WebVTT/SRT basics are implemented at the
  `VideoController` text-track layer, and `VideoPlayer::source(...)` renders
  selected text cues with customizable caption styling and built-in caption
  track selection. Native audio/video stream selection remains roadmap work.
- Streaming reality: progressive HTTP first; HLS/DASH either native-backed or
  automatically routed through WebView by `VideoPlayerRoute::Auto` until native
  support exists. `VideoPlaybackRoute` helpers flag HLS/DASH manifests and
  adaptive streaming MIME types as WebView-recommended. Initial `VideoCanPlay`
  helpers mirror
  `canPlayType`-style confidence for MIME types and sources, and
  `webview_video_player_url(...)` creates a WebView-hosted browser video page
  for URL/file fallbacks that posts media events back to `VideoPlayer`.
- Backend ladder: current FFmpeg software decode first, stream-level seek and
  prefetch, then platform hardware decode with low-copy or zero-copy textures.

### Media implementation slices

1. Add `VideoController`/`MediaController` as the stateful owner of playback.
   Initial `VideoController` is implemented.
2. Add `VideoHandle` commands and `VideoEvent` notifications. Initial command
   and event surface is implemented on `VideoController`, including
   source-change, ready-state, and buffered-range events.
3. Make `VideoPlayer::source(...)` create and wire a controller automatically.
   Initial wrapper is implemented.
4. Add source-backed `VideoPlayer` builder methods for common video-element
   attributes such as controls, autoplay, volume, muted, playback rate, loop,
   preload, SRT/WebVTT text tracks, selected captions, caption styling,
   object-fit, poster, and initial seek. Initial attribute-like configuration
   and built-in caption selection are implemented.
5. Keep `VideoPlayer::new(state)` for custom/legacy control overlays.
6. Move decode, buffering, and clock management off paint-time paths.
7. Add true seek on `VideoFrameStream` instead of restart-on-backward-position.
   Initial FFmpeg stream seek is implemented; exact-frame seeking still depends
   on decoding forward from the nearest keyframe.
8. Add platform surface backends:
   - macOS: AVFoundation/CoreVideo/Metal.
   - Windows: Media Foundation/D3D texture.
   - Linux: GStreamer/VAAPI/DMABUF where available.
9. Render selected text-track cues over `VideoPlayer::source(...)` with
   customizable caption styling. Implemented.

## WebView islands are a feature, not a failure

Some requirements are web-shaped: maps, hosted payments, rich third-party
editors, SSO flows, complex video streaming, embedded documentation,
browser-only graphics, and customer-provided web widgets. Kael should make
these first-class through `webview(id, url)`, `webview_with_options(...)`,
`WebViewOptions`, `webview_controller(id)`, JavaScript evaluation, injected
CSS/JS, message passing, and navigation handlers.

```rust
use kael::{
    webview_controller, webview_file_with_options, webview_html_with_options, webview_with_options,
    NavigationPolicy, WebViewBridgeMessage, WebViewDownloadPolicy, WebViewDragDropPolicy,
    WebViewNewWindowPolicy, WebViewOptions, WebViewPageLoadEvent,
};

let browser = webview_controller("checkout");
let downloads_dir = std::env::current_dir()?.join("downloads");
std::fs::create_dir_all(&downloads_dir)?;
let mut auth_headers = http_client::http::HeaderMap::new();
auth_headers.insert(
    http_client::http::header::AUTHORIZATION,
    http_client::http::HeaderValue::from_static("Bearer preview-token"),
);

div().child(
    webview_with_options(
        browser.id(),
        checkout_url,
        WebViewOptions::embedded_widget()
            .user_agent("MyApp/1.0")
            .devtools()
            .zoom_hotkeys()
            .media_autoplay()
            .focused()
            .clipboard_access()
            .transparent_background()
            .request_headers(auth_headers.clone())
            .general_autofill_enabled(false)
            .bridge_script()
            .on_bridge_message({
                let browser = browser.clone();
                move |message, window, _cx| {
                    if message.is_kind("pick-video") {
                        browser.respond_to_bridge_message(
                            window,
                            &message,
                            serde_json::json!({ "path": "/movies/trailer.mp4" }),
                        )
                        .ok();
                    }
                }
            })
            .on_bridge_message(|message, _window, _cx| {
                if message.is_kind("checkout-complete") {
                    println!("checkout payload: {}", message.payload);
                }
            })
            .on_navigate(|url, _window, _cx| {
                if url.starts_with("https://trusted.example") {
                    NavigationPolicy::Allow
                } else {
                    NavigationPolicy::Deny
                }
            })
            .on_new_window(|url, _window, _cx| {
                if url.starts_with("https://trusted.example") {
                    WebViewNewWindowPolicy::NavigateCurrent
                } else {
                    WebViewNewWindowPolicy::Deny
                }
            })
            .on_download_started({
                let downloads_dir = downloads_dir.clone();
                move |url, suggested_path, _window, _cx| {
                    if url.starts_with("https://trusted.example/reports/") {
                        let filename = suggested_path
                            .and_then(|path| path.file_name().map(|name| name.to_owned()))
                            .unwrap_or_else(|| "download.bin".into());
                        WebViewDownloadPolicy::SaveTo(downloads_dir.join(filename))
                    } else {
                        WebViewDownloadPolicy::Deny
                    }
                }
            })
            .on_download_completed(|download, _window, _cx| {
                println!(
                    "download {}: {:?}",
                    if download.success { "complete" } else { "failed" },
                    download.path
                );
            })
            .on_drag_drop(|event, _window, _cx| {
                println!("webview file drag/drop event: {event:?}");
                WebViewDragDropPolicy::AllowBrowserDefault
            })
            .on_document_title_changed(|title, window, _cx| {
                window
                    .set_window_title_checked(WindowTitleBuilder::new(format!(
                        "Checkout - {title}"
                    )))
                    .expect("validated checkout title");
            })
            .on_page_load(|event, url, _window, _cx| {
                match event {
                    WebViewPageLoadEvent::Started => println!("loading {url}"),
                    WebViewPageLoadEvent::Finished => println!("loaded {url}"),
                }
            }),
    )
    .size_full(),
);

browser.navigate_with_headers(window, checkout_url, auth_headers)?;
browser.post_bridge_message(window, WebViewBridgeMessage::new("host-ready"))?;
browser.open_devtools(window)?;
browser.is_devtools_open(window, |result| {
    println!("devtools open: {result:?}");
})?;
browser.set_zoom_factor(window, 1.1)?;
browser.focus(window)?;
browser.evaluate_javascript(window, "window.dispatchEvent(new Event('host-ready'))")?;
browser.evaluate_javascript_with_result(window, "document.title", |result| {
    println!("document title result: {result:?}");
})?;
browser.url(window, |result| {
    println!("current WebView URL: {result:?}");
})?;
browser.reload(window)?;
browser.print(window)?;
browser.clear_browsing_data(window)?; // Logout, account switching, or test cleanup.
```

For app-rendered documents, use the native print request path instead of
round-tripping through HTML:

```rust
let job = PrintJob::letter("Invoice", |ctx, cx| {
    ctx.draw_text(
        "Invoice #1042",
        point(px(72.0), px(72.0)),
        PrintTextStyle::default(),
    );
});

window.print_checked(PrintRequest::dialog(job), cx)?;
```

`PrintRequest::dialog(job)` is the safe default because it shows the platform
print UI. Use `PrintRequest::silent(job)` only for deliberate direct printer
dispatch, and `PrintRequest::webview(id)` when an existing WebView-hosted
document should follow Electron `webContents.print(...)` behavior. The checked
path validates native print titles, pages, page sizes, margins, drawing
commands, and WebView ids before dispatch.

For a local HTML file, use `webview_file(...)` /
`webview_file_with_options(...)`, the native analogue of Electron's `loadFile`:

```rust
webview_file_with_options(
    "local-docs",
    "assets/docs/index.html",
    WebViewOptions::embedded_widget()
        .bridge_script()
        .allow_navigation_schemes(["file", "data", "https"]),
)?
.size_full();
```

For controlled browser islands that do not need a separate server or asset file,
use `webview_html(...)` / `webview_html_with_options(...)`, the native analogue
of Electron's `loadHTML`. These load the HTML string directly into the native
WebView; use `webview_html_url(...)` only when you explicitly need a data URL
string for another API:

```rust
let preview = webview_controller("preview");

webview_html_with_options(
    preview.id(),
    r#"<!doctype html>
<button onclick="window.kael.post('clicked')">Click</button>"#,
    WebViewOptions::embedded_widget()
        .bridge_script()
        .on_bridge_message(|message, _window, _cx| {
            if message.is_kind("clicked") {
                println!("inline widget clicked");
            }
        }),
)
.size_full();
```

Use `WebViewController::load_html(window, html)` when an existing browser island
should replace its document at runtime, such as live previews, generated
reports, template editors, or local documentation panes.

When you control the page, inject `.bridge_script()` and use
`window.kael.post(kind, payload, id)` for fire-and-forget messages or
`await window.kael.invoke(kind, payload)` for request/response calls. On the
Rust side, `WebViewBridgeMessage { kind, id, payload }`,
`.on_bridge_message(...)`, `WebViewController::post_bridge_message(...)`,
`WebViewController::respond_to_bridge_message(...)`, and
`WebViewController::reject_bridge_message(...)` give builders the same
message-envelope habit they expect from Electron IPC. Raw `.on_message(...)` /
`.post_message(...)` remain available for custom protocols; lower-level
`WebViewBridgeMessage::response_to(...)` and `.error_to(...)` remain available
when a custom router owns the controller.

When the native app needs Electron-style `webContents.executeJavaScript(...)`,
use `WebViewController::evaluate_javascript_with_result(window, script,
callback)`. Linux/Windows Wry-backed WebViews return the backend's JSON string
serialization of the JavaScript result through `Result<SharedString,
SharedString>`, so app code can decide whether to parse JSON or keep the raw
browser value. The existing `evaluate_javascript(...)` remains available for
fire-and-forget commands. Kael's custom macOS WebView backend uses WKWebView's
JavaScript completion handler and returns a JSON string serialization as well.

For Electron-style `webContents.getURL()`, use
`WebViewController::url(window, callback)`. Linux/Windows Wry-backed WebViews
return Wry's current URL, and Kael's custom macOS WebView backend reads
`WKWebView.URL.absoluteString`. Before a macOS WebView has committed a page,
Kael falls back to the URL last declared through Kael navigation/load APIs.

During development, request inspector support with `.devtools()`. On
Linux/Windows debug/devtools builds, `WebViewController::open_devtools(window)`,
`.close_devtools(window)`, and `.is_devtools_open(window, callback)` map to
Wry-backed WebView devtools controls. On macOS, `.devtools()` marks the
underlying `WKWebView` as inspectable so it can appear in Safari/Web Inspector,
and `open_devtools(window)` ensures inspectability is enabled. WKWebView does
not expose public APIs to programmatically open, close, or report the inspector
window state, so custom devtool chrome should treat those runtime controls as
Linux/Windows-only for now.

For app-owned diagnostics, hosted-widget health checks, automated tests, and
AI-agent observability, use
`.on_console_message("console:message", |event, window, cx| { ... })`. It
preserves normal browser console behavior while forwarding typed
`WebViewConsoleEvent { level, message, args, source, line, column }` values for
`console.debug/log/info/warn/error`, uncaught errors, and unhandled promise
rejections. Use `.console_bridge("console:message")` with
`.on_bridge_message(...)` plus
`WebViewConsoleEvent::from_bridge_message(&message, "console:message")` when a
custom router owns multiple bridge message kinds.

For hosted documents, maps, editors, and dashboards that should own browser
zoom shortcuts, request backend zoom handling with `.zoom_hotkeys()` or
`.zoom_hotkeys_enabled(true)`. This maps to Wry's browser zoom hotkey/gesture
setting; Windows/WebView2 honors it today, and Kael's custom macOS WebView
backend handles standard `Command` + `+`, `Command` + `-`, and `Command` + `0`
keyboard zoom plus trackpad magnification gestures through WKWebView
`pageZoom`. Linux does not expose this behavior through the backend API yet. Use
`WebViewController::set_zoom_factor(window, factor)` when the native app should
drive zoom through its own controls instead; Linux/Windows Wry-backed WebViews
and Kael's custom macOS WKWebView backend honor that runtime command.

For native chrome that needs to observe hosted editor shortcuts or browser
before-input activity, use
`.on_keyboard_event("keyboard:event", |event, window, cx| { ... })`. It
forwards typed `WebViewKeyboardEvent` values for `keydown`, `keyup`, and
`beforeinput` with key/code, modifiers, repeat/composition state, editable-target
state, input type, data, and `defaultPrevented`. This is a portable WebView
island bridge for Electron-style `before-input-event` diagnostics and shortcut
coordination. Commands that must cancel input before a page sees it should still
use native Kael shortcut/keymap handling around the WebView boundary.

For native tabs, breadcrumbs, app-owned Back/Forward controls, and agents that
need hosted SPA route awareness, use
`.on_location_changed("location:changed", |event, window, cx| { ... })`. It
injects the standard bridge and forwards typed
`WebViewLocationEvent { url, title, ready_state, can_go_back, can_go_forward }`
values on `pushState`, `replaceState`, `popstate`, `hashchange`, `pageshow`, DOM
ready, load, and title-related DOM mutations. Use
`.location_bridge("location:changed")` with `.on_bridge_message(...)` plus
`WebViewLocationEvent::from_bridge_message(&message, "location:changed")` when a
custom router owns multiple bridge message kinds. Pair it with
`.navigation_state_bridge()` when native Forward state should be accurate for
app-owned same-document routes; otherwise `can_go_forward` stays conservative.

For resource throttling, active-tab chrome, hosted-player pause/resume, and
automation that needs to know whether browser content is active, use
`.on_lifecycle_event("lifecycle:event", |event, window, cx| { ... })`. It
forwards typed
`WebViewLifecycleEvent { event, visibility_state, hidden, has_focus, fullscreen, persisted }`
values for `focus`, `blur`, `visibilitychange`, `pageshow`, `pagehide`, and
browser fullscreen changes. Use `.lifecycle_bridge("lifecycle:event")` with
`.on_bridge_message(...)` plus
`WebViewLifecycleEvent::from_bridge_message(&message, "lifecycle:event")` when a
custom router owns multiple bridge message kinds. This is the portable
browser-side companion to native window focus and visibility handling; app code
should still use native Kael window lifecycle hooks for whole-window state.

For hosted documents, readers, dashboards, and editor panes whose scroll
position should drive native chrome or automation, use
`.on_scroll_event("scroll:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewScrollEvent { event, x, y, max_x, max_y, viewport_width, viewport_height, scroll_width, scroll_height, progress_x, progress_y }`
values for initial, scroll, and viewport resize snapshots. Use
`.scroll_bridge("scroll:event")` with `.on_bridge_message(...)` plus
`WebViewScrollEvent::from_bridge_message(&message, "scroll:event")` when a
custom router owns multiple bridge message kinds. The script throttles updates
with `requestAnimationFrame`, so native progress bars, hiding toolbars, and
AI-agent viewport checks can observe browser scroll state without polling.

For hosted rich editors, documents, and preview panes whose selection should
drive native edit menus or floating formatting chrome, use
`.on_selection_event("selection:event", |event, window, cx| { ... })`. It
forwards typed
`WebViewSelectionEvent { event, selected_text, selected_html, collapsed, editable, input_kind }`
values for initial, document `selectionchange`, input `select`, key/mouse/touch
selection updates, and focus/blur snapshots. Use
`.selection_bridge("selection:event")` with `.on_bridge_message(...)` plus
`WebViewSelectionEvent::from_bridge_message(&message, "selection:event")` when a
custom router owns multiple bridge message kinds. This is the event-driven
companion to `selected_text(...)` and `selected_html(...)`: native Edit,
Format, Copy/Cut/Paste, and AI-agent inspection flows can react as the browser
selection changes.

For browser-media islands, demos, and hosted players that should start media
without a user gesture, request the browser autoplay policy explicitly with
`.media_autoplay()` or `.media_autoplay_enabled(true)`. Use
`.media_autoplay_enabled(false)` when an embedded third-party page should keep a
stricter gesture requirement. Linux/Windows Wry-backed WebViews and Kael's
custom macOS WKWebView backend honor this construction option today.

For custom native controls around WebView-hosted `<audio>` or `<video>`, prefer
`.on_media_event("media:event", |event, window, cx| { ... })`. It injects the
bridge, observes current and future media elements, and passes typed
`WebViewMediaEvent { event, state }` values for play/pause/seek/time/volume/
rate/metadata/buffering/error events. Use `.media_event_bridge("media:event")`
with `.on_bridge_message(...)` plus
`WebViewMediaEvent::from_bridge_message(&message, "media:event")` when you need
to share one bridge handler across several message kinds. Use
`webview_media_event_bridge_script(kind)` directly only when composing a custom
injection bundle.

For native right-click / secondary-click menus over WebView content, prefer
`.on_context_menu("context:menu", |event, window, cx| { ... })`. It injects the
standard bridge, prevents the browser default context menu, and passes typed
`WebViewContextMenuEvent` values with viewport coordinates, selected text,
nearest link href, image source, media source, editable state, and input kind.
Use `.context_menu_bridge("context:menu")` with `.on_bridge_message(...)` plus
`WebViewContextMenuEvent::from_bridge_message(&message, "context:menu")` when
one bridge handler needs to route multiple message kinds. This is the
Electron-style path for app-owned context menus around hosted editors,
documents, media previews, and browser widgets: collect page context from the
WebView, then call the native context-menu builder from the handler.

For hover status bars, link previews, click telemetry, and AI-agent pointer
inspection over hosted content, use
`.on_pointer_event("pointer:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewPointerEvent { event, x, y, buttons, pointer_type, target_tag, link_href, image_src, media_src, editable, input_kind }`
values for pointer movement, pointer down/up, click, double-click, and pointer
leave. Use `.pointer_bridge("pointer:event")` with `.on_bridge_message(...)`
plus `WebViewPointerEvent::from_bridge_message(&message, "pointer:event")` when
a custom router owns multiple bridge message kinds. This is the lightweight
hover/click companion to `.on_context_menu(...)`; it does not prevent browser
defaults.

For hosted auth, checkout, settings, admin, and browser-widget forms, use
`.on_form_event("form:event", |event, window, cx| { ... })`. It forwards typed
`WebViewFormEvent { event, form_id, form_name, action, method, target, enctype, field, fields, default_prevented }`
values for submit, reset, change, and input events. Field snapshots include
name, id, tag, input kind, non-sensitive value, checked state, disabled state,
and required state; password and file input values are intentionally omitted.
Use `.form_bridge("form:event")` with `.on_bridge_message(...)` plus
`WebViewFormEvent::from_bridge_message(&message, "form:event")` when a custom
router owns multiple bridge kinds. This gives native validation, progress
chrome, tests, and AI agents a structured form surface without bespoke page
JavaScript, and it does not prevent browser defaults.

For hosted upload flows that use `<input type="file">`, add
`.on_file_input_event("file:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewFileInputEvent { event, input_name, input_id, accept, multiple, form_id, form_name, action, method, files }`
values when file inputs emit `change` or `input`. Each file entry includes the
browser-exposed file name, size, MIME type, and last-modified timestamp. Local
paths are not exposed by browsers, so this bridge is for native upload chrome,
validation, tests, and AI-agent observability rather than path access. Use
`.file_input_bridge("file:event")` with `.on_bridge_message(...)` plus
`WebViewFileInputEvent::from_bridge_message(&message, "file:event")` when a
custom router owns multiple bridge kinds.

For native diagnostics, loading UI, tests, and AI-agent observability around
hosted subresources, use
`.on_resource_event("resource:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewResourceEvent { event, url, initiator_type, target_tag, success, start_time, duration, transfer_size, encoded_body_size, decoded_body_size, next_hop_protocol, render_blocking_status }`
values from browser `PerformanceResourceTiming` entries plus captured element
`load` / `error` events. Use `.resource_bridge("resource:event")` with
`.on_bridge_message(...)` plus
`WebViewResourceEvent::from_bridge_message(&message, "resource:event")` when a
custom router owns multiple bridge kinds. This is resource observability, not
request interception: use it to see what loaded or failed, while main-frame
headers/navigation remain controlled by `.request_headers(...)`,
`navigate_with_headers(...)`, and `.on_navigate(...)`.

For hosted apps that call `fetch(...)` or `XMLHttpRequest`, use
`.on_network_event("network:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewNetworkEvent { event, api, method, url, status, status_text, ok, duration_ms, error_name, error_message, response_type, document_url }`
values for fetch completion/rejection and XHR load/error/abort/timeout. Use
`.network_bridge("network:event")` with `.on_bridge_message(...)` plus
`WebViewNetworkEvent::from_bridge_message(&message, "network:event")` when a
custom router owns multiple bridge kinds. This is JavaScript network API
observability, not request interception: it cannot rewrite headers, block
requests, inspect bodies, or observe browser requests that do not go through
page `fetch`/XHR.

For hosted pages that may call `alert`, `confirm`, `prompt`, or register
`beforeunload` prompts, use
`.on_dialog_event("dialog:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewDialogEvent { event, message, default_value, result, url, default_prevented }`
values after the browser produces its normal synchronous dialog result. Use
`.dialog_bridge("dialog:event")` with `.on_bridge_message(...)` plus
`WebViewDialogEvent::from_bridge_message(&message, "dialog:event")` when a
custom router owns multiple bridge kinds. This is dialog observability, not a
replacement for the browser's synchronous `confirm()` / `prompt()` return path.

For auth, checkout, editor, and browser-widget islands that should take
keyboard focus immediately, request initial focus with `.focused()` or
`.focused_enabled(true)`. Use `WebViewController::focus(window)` and
`.focus_parent(window)` to move focus across the native/WebView boundary after
modals, route changes, completed auth, or embedded-editor handoff. Wry-backed
Linux/Windows WebViews and Kael's custom macOS WKWebView backend honor these
focus commands today.

For rich hosted editors, document widgets, and browser pages that call
`navigator.clipboard` or `document.execCommand("copy")`, request JavaScript
clipboard access with `.clipboard_access()` or
`.clipboard_access_enabled(true)`. Wry-backed Linux/Windows WebViews honor this
construction option today. Kael's custom macOS WebView backend injects an
opt-in native bridge for `navigator.clipboard.readText()` and
`navigator.clipboard.writeText(...)` backed by `NSPasteboard`, supports the
`text/plain` subset of `navigator.clipboard.read()` / `write(...)`, and maps
legacy `document.execCommand("copy")` / `"cut"` text selection calls onto the
same bridge. Broader clipboard item MIME types remain controlled by WebKit,
macOS permissions, and app menu accelerators.

When native chrome, tests, or AI agents need to observe clipboard activity
inside hosted editors, add
`.on_clipboard_event("clipboard:event", |event, window, cx| { ... })`. It
forwards typed
`WebViewClipboardEvent { event, types, text, html, target_editable, url, default_prevented }`
values for browser `copy`, `cut`, and `paste` events when the browser exposes
clipboard data to the page event. Use `.clipboard_event_bridge("clipboard:event")`
with `.on_bridge_message(...)` plus
`WebViewClipboardEvent::from_bridge_message(&message, "clipboard:event")` when
a custom router owns multiple bridge kinds. This bridge does not prevent
browser defaults and should be treated as explicit opt-in because paste payloads
can contain user data.

For hosted calls, screen-share flows, maps, local-device widgets, and other
pages that may request camera, microphone, display capture, geolocation, or
notification access, add
`.on_permission_request("permission:request", |request, window, cx| { ... })`.
It forwards typed
`WebViewPermissionRequest { permission, permissions, api, url, origin, user_gesture, details }`
values before wrapped browser APIs continue. Return
`WebViewPermissionDecision::Deny` to block the page call before it reaches the
browser, or `Allow` / `Default` to continue to the embedded browser's native
permission flow. Use `.permission_bridge("permission:request")` with
`.on_bridge_message(...)` plus
`WebViewPermissionRequest::from_bridge_message(&message, "permission:request")`
when a custom router owns multiple bridge kinds. This is an app policy
preflight for WebView islands; the browser engine and operating system remain
the final authority for native prompts.

For hosted auth, settings, carts, drafts, and embedded widgets that use Web
Storage, add `.on_storage_event("storage:event", |event, window, cx| { ... })`.
It forwards typed
`WebViewStorageEvent { event, area, key, old_value, new_value, length, url, local }`
values when hosted content mutates `localStorage` or `sessionStorage`, and when
the browser emits cross-document `storage` events. Use
`.storage_bridge("storage:event")` with `.on_bridge_message(...)` plus
`WebViewStorageEvent::from_bridge_message(&message, "storage:event")` when a
custom router owns multiple bridge kinds. For on-demand inspection and setup,
use `WebViewController::storage_snapshot(window, callback)`,
`.set_storage_item(window, WebViewStorageArea::Local, key, value, callback)`,
`.remove_storage_item(window, WebViewStorageArea::Session, key, callback)`, and
`.clear_storage_area(window, area, callback)`. Snapshot callbacks report
readable entries plus per-area `available` / `error` fields, and mutation
callbacks report `WebViewStorageMutationResult { ok, area, key, length, error }`.
These helpers are storage observability and current-document mutation, not a
replacement for the browser storage engine; use controller
`clear_browsing_data(window)` for profile cleanup.

For native-looking browser islands, set the host surface with
`.background_color(color)` or request `.transparent_background()`. This is
useful for inline previews, shaped widgets, WebGL/canvas overlays, and embeds
that should inherit native chrome instead of showing a default white rectangle.
Linux/Windows Wry-backed WebViews honor this construction option and runtime
updates today; Kael's custom macOS WKWebView backend honors it for the native
WebView host surface as well.

For untrusted static docs, preview panes, or sanitised customer-provided HTML
that should not execute scripts, use `.javascript_disabled()` or
`.javascript_disabled_enabled(true)`. Linux/Windows Wry-backed WebViews and
Kael's custom macOS WKWebView backend honor this construction option today. Do
not combine this with
`.bridge_script()`, `.inject_javascript(...)`, or hosted widgets that require
page JavaScript.

For hosted account forms, profile pages, and privacy-sensitive embeds on
Windows, tune browser-level general form suggestions with
`.general_autofill_enabled(false)` or `.general_autofill_disabled()`. This maps
to WebView2's general autofill setting and does not disable password or
credit-card autofill. Wry reports this option as unsupported on Linux/macOS, so
Kael treats it as a Windows-only WebView preference today.

For hosted pages that call `window.open(...)` or use `target="_blank"`, set an
explicit new-window policy. `.deny_new_windows()` blocks popups,
`.open_new_windows_in_current_webview()` keeps the flow inside the current
island, `.allow_new_windows()` delegates to the backend default, and
`.on_new_window(...)` lets the app choose per URL with
`WebViewNewWindowPolicy::{Deny, NavigateCurrent, Allow}`. Wry-backed
Linux/Windows WebViews honor this policy today. Kael's custom macOS WKWebView
backend honors all three policies for target-blank requests. On macOS, `Allow`
creates a WebKit-managed popup child WebView inside the same native window;
prefer `NavigateCurrent` or a custom handler when the app should own the
resulting route, chrome, or window lifecycle.

For authenticated previews, localized docs, test fixtures, and hosted tools
that need request metadata, use `.request_headers(headers)` on
`WebViewOptions` for the first load and
`WebViewController::navigate_with_headers(window, url, headers)` for later
navigations. Both accept `http_client::http::HeaderMap`. This is the
browser-island equivalent of Electron `loadURL(url, { extraHeaders })`: it
applies to the main navigation request, while subresource requests remain
controlled by the embedded browser engine. Linux/Windows Wry-backed WebViews
and Kael's custom macOS WKWebView backend honor request headers today.

For pages that trigger browser downloads, set a download policy too.
`.allow_downloads()` keeps the backend default, `.deny_downloads()` blocks all
downloads, and `.on_download_started(...)` can return
`WebViewDownloadPolicy::{Allow, Deny, SaveTo(path)}`. `SaveTo` must use an
absolute destination path because that is what the Wry backends require.
`.on_download_completed(...)` receives `WebViewDownloadCompleted { url, path,
success }` for progress handoff into the native app. Linux/Windows Wry-backed
WebViews and Kael's custom macOS WKWebView backend honor these handlers today.
On macOS, `Allow` resolves to the user's `~/Downloads` folder using WebKit's
suggested filename, while `SaveTo(path)` still requires an absolute path.
When a native context menu, command palette, or agent receives a `linkHref`,
`imageSrc`, or `mediaSrc` from the WebView bridges, call
`WebViewController::trigger_download(window, url, filename, callback)` or the
alias `download_url(...)` to dispatch a browser `<a download>` action inside
the hosted document. The callback returns `WebViewDownloadTriggerResult` with
the resolved URL and requested filename hint; the final destination and success
still come from the download policy and completion handlers. Cross-origin
responses and `Content-Disposition` headers may ignore the filename hint.

For app-owned downloads that do not start in browser content, use a checked
`DownloadRequest` instead of routing through a hidden WebView. This covers
exports, offline packs, model/artifact fetches, plugin assets, installer
helpers, and background worker queues:

```rust
let request = DownloadRequest::builder(url, destination)
    .network_policy(policy)
    .sha256(expected_sha256)
    .size_bytes(expected_size)
    .create_parent_dirs()
    .build_checked()?;
```

`DownloadRequest` rejects empty or non-HTTP(S) URLs, missing hosts, relative or
directory destinations, invalid SHA-256 or zero sizes, missing parent
directories unless `.create_parent_dirs()` is set, and URLs denied by the
attached `NetworkPolicy`. The descriptor is transport-agnostic: hand it to an
HTTP client, worker pool, plugin host, or export manager after validation.

For app-owned HTTP requests that are not downloads, use
`AppNetworkRequestBuilder` as the Electron `net.request`-style descriptor before
handing work to the app HTTP client:

```rust
let request = AppNetworkRequestBuilder::post("https://api.example.com/v1/sync")
    .header("Content-Type", "application/json")
    .body_size_bytes(512)
    .network_policy(policy)
    .build_checked()?;
```

The descriptor validates HTTP(S) URLs, host policy, method/body shape, duplicate
or malformed headers, and CR/LF header injection. It stays transport-agnostic:
the app still chooses the HTTP client, retry policy, body bytes, and response
handling.

For Electron `WebSocket` and `EventSource` parity outside hosted browser pages,
use `AppRealtimeConnection` as the checked realtime descriptor:

```rust
let realtime = AppRealtimeConnection::websocket("wss://events.example.com/socket")
    .protocol("kael.v1")
    .heartbeat_interval(std::time::Duration::from_secs(30))
    .network_policy(policy)
    .build_checked()?;
```

Use `AppRealtimeConnection::server_sent_events(url)` for EventSource-style
streams. The descriptor validates transport-specific URL schemes, host policy,
headers, WebSocket subprotocols, heartbeat intervals, and inbound message
budgets before the app opens its chosen realtime client.

For pages that accept dragged files, use `.on_drag_drop(...)` to observe file
drag/drop events entering, moving over, dropping on, or leaving the WebView.
Return `WebViewDragDropPolicy::AllowBrowserDefault` when the page should keep
normal browser behavior, including drops onto `<input type="file">`. Return
`WebViewDragDropPolicy::BlockBrowserDefault`, or use `.block_drag_drop()`, when
hosted content should not receive local file drops. Linux/Windows Wry-backed
WebViews and Kael's custom macOS WKWebView backend honor this handler today.
On macOS, returning `AllowBrowserDefault` forwards the drag/drop operation to
WebKit so browser inputs and page handlers can still receive it; returning
`BlockBrowserDefault` prevents WebKit's default handling after the Kael handler
runs.

For hosted docs, auth, checkout, and editor islands that change
`document.title`, use `.on_document_title_changed(...)` to synchronize native
window titles, tabs, breadcrumbs, or inspector labels. Linux/Windows Wry-backed
WebViews and Kael's custom macOS WKWebView backend honor this handler today.
Prefer `window.set_window_title_checked(WindowTitleBuilder::new(title))?` when
the title comes from generated code, hosted pages, documents, or routes; the
checked path rejects empty, padded, control-character, and overly long platform
chrome text. Raw `window.set_window_title(...)` remains available for already
validated titles.
For hosted pages that expose favicons, use
`WebViewController::favicons(window, callback)` for an on-demand snapshot or
`.on_favicon_changed("favicon:changed", |event, window, cx| { ... })` for
event-driven native tab icons. The bridge reports resolved URLs from
`<link rel="icon">`, shortcut icons, Apple touch icons, and mask icons; it does
not fetch or decode image bytes for the app.

For browser content that should use familiar web commands, keep the
`WebViewController` next to the element id. In addition to navigation, reload,
JavaScript evaluation, and bridge messages, the controller exposes
`.navigate_with_headers(window, url, headers)`, `.load_html(window, html)`, `.focus(window)`,
`.focus_parent(window)`, `.set_zoom_factor(window, factor)`, `.print(window)`,
`.insert_css(window, key, css)`, `.remove_inserted_css(window, key)`,
`.find_text(window, query, WebViewFindOptions::forward(), callback)`,
`.find_text_result(window, query, WebViewFindOptions::forward(), callback)`,
`.stop_finding(window)`,
`.stop_finding_with_action(window, WebViewStopFindAction::KeepSelection)`,
`.copy(window, callback)`, `.cut(window, callback)`,
`.paste(window, callback)`, `.select_all(window, callback)`,
`.undo(window, callback)`, `.redo(window, callback)`,
`.delete_selection(window, callback)`,
`.insert_text(window, text, callback)`,
`.focus_selector(window, selector, callback)`,
`.click_selector(window, selector, callback)`,
`.add_class(window, selector, class_name, callback)`,
`.remove_class(window, selector, class_name, callback)`,
`.toggle_class(window, selector, class_name, force, callback)`,
`.set_attribute(window, selector, name, value, callback)`,
`.remove_attribute(window, selector, name, callback)`,
`.set_style_property(window, selector, name, value, callback)`,
`.remove_style_property(window, selector, name, callback)`,
`.set_form_value(window, selector, value, callback)`,
`.submit_form(window, selector, callback)`,
`.reset_form(window, selector, callback)`,
`.selected_text(window, callback)`,
`.selected_html(window, callback)`,
`.document_html(window, callback)`,
`.document_snapshot(window, callback)`,
`.element_snapshot(window, selector, callback)`,
`.capture_dom_image(window, selector, options, callback)`,
`.trigger_download(window, url, filename, callback)`,
`.download_url(window, url, filename, callback)`,
`.favicons(window, callback)`,
`.edit_command(window, WebViewEditCommand::Copy, callback)`,
`.title(window, callback)`,
`.user_agent(window, callback)`,
`.is_loading(window, callback)`,
`.can_go_back(window, callback)`, `.can_go_forward(window, callback)`,
`.viewport_snapshot(window, callback)`,
`.scroll_to(window, x, y, callback)`, `.scroll_by(window, dx, dy, callback)`,
`.scroll_selector_into_view(window, selector, callback)`,
`.cookies(window, callback)`, `.cookies_for_url(window, url, callback)`,
`.set_cookie(window, cookie, callback)`, `.delete_cookie(window, cookie, callback)`,
`.storage_snapshot(window, callback)`,
`.set_storage_item(window, area, key, value, callback)`,
`.remove_storage_item(window, area, key, callback)`,
`.clear_storage_area(window, area, callback)`,
`.stop_loading(window)`, `.play_media(window)`, `.pause_media(window)`,
`.set_media_muted(window, muted)`, `.set_media_volume(window, volume)`,
`.set_media_playback_rate(window, rate)`, `.seek_media_secs(window, seconds)`,
`.media_command(window, selector, command, callback)`,
`.set_media_source(window, selector, source, callback)`,
`.set_media_options(window, selector, options, callback)`,
`.capture_media_frame(window, selector, options, callback)`,
`.add_media_text_track(window, selector, track, callback)`,
`.remove_media_text_track(window, selector, track_selector, callback)`,
`.select_media_text_track(window, selector)`,
`.disable_media_text_tracks(window)`,
`.request_media_fullscreen(window)`, `.exit_media_fullscreen(window)`,
`.request_media_picture_in_picture(window)`,
`.exit_media_picture_in_picture(window)`,
`.media_state(window, callback)`, `.mute_media(window)`, `.unmute_media(window)`, and
`.clear_browsing_data(window)` for
`webContents.loadURL(...)` with extra
headers, `webContents.loadHTML(...)`, `webContents.focus()`,
`webContents.setZoomFactor(...)`, `webContents.print(...)`,
runtime `webContents.insertCSS(...)` / `removeInsertedCSS(...)` styling,
basic `webContents.findInPage(...)` / `stopFindInPage(...)` flows,
`webContents.copy()` / `cut()` / `paste()` / `selectAll()` / `undo()` /
`redo()` edit flows, `webContents.insertText(...)` hosted-editor typing,
selector-driven focus/click helpers for hosted controls, selector-driven hosted
DOM class/attribute/style customization, form value setting, and form
submission,
`webContents.getSelectedText()`, rich selection HTML inspection, common
`executeJavaScript("document.documentElement.outerHTML")` export/diagnostic
flows, selector-scoped element inspection,
`webContents.getTitle()`,
Electron-style page favicon update flows for native tabs,
`webContents.getUserAgent()`,
`webContents.isLoading()`,
`webContents.canGoBack()` / `webContents.canGoForward()`,
hosted document viewport inspection and app-owned scrolling,
`webContents.stop()`,
app-owned play/pause/mute/volume/rate/seek controls and state snapshots for
browser `<audio>` and `<video>` elements, including browser fullscreen and
picture-in-picture requests,
`session.cookies.get(...)`, `session.cookies.set(...)`,
`session.cookies.remove(...)`, local/session Web Storage inspection and seeding,
and session cleanup workflows. Read callbacks
receive `Result<Vec<WebViewCookie>, SharedString>` with name, value, domain,
path, secure, and http-only metadata. Set/delete callbacks receive
`Result<(), SharedString>`. `storage_snapshot(...)` returns
`Result<WebViewStorageSnapshot, SharedString>` with URL, origin, readable
`localStorage` entries, and readable `sessionStorage` entries.
`set_storage_item(...)`, `remove_storage_item(...)`, and
`clear_storage_area(...)` take `WebViewStorageArea::{Local, Session}` and return
`Result<WebViewStorageMutationResult, SharedString>` with `ok`, area, key,
length, and browser error text when the current document cannot access storage.
Use these helpers for auth/session debugging, hosted settings, carts, draft
seeding, tests, and AI-agent state inspection without custom page JavaScript.
Browser origin, sandboxing, private-mode, and storage quota rules still apply;
for full profile cleanup use `clear_browsing_data(window)`. Find callbacks receive `Result<bool, SharedString>`
and report whether the browser found and selected a match. Use
`.find_text_result(...)` when native find chrome also needs a result count; it
returns `Result<WebViewFindResult, SharedString>` with `found` plus a portable
DOM text match count for the current document. That count does not inspect
cross-origin frames or backend-native hidden match state. Use
`.find_result_bridge("find:result")` or
`.on_find_result("find:result", |event, window, cx| { ... })` when native find
chrome, tests, or agents need Electron-style `found-in-page` updates after
browser `window.find(...)` calls, including the query, options, found flag,
match count, selected text, and page URL. `stop_finding(window)` maps to
Electron's clear-selection default, and
`stop_finding_with_action(...)` accepts
`WebViewStopFindAction::{ClearSelection, KeepSelection, ActivateSelection}` for
Electron-style `stopFindInPage(action)` find-bar behavior.
Edit-command callbacks also receive `Result<bool, SharedString>` with the
browser's `document.execCommand(...)` success flag. `insert_text(...)` returns
`Result<bool, SharedString>` and tries browser `insertText` first, then falls
back to replacing the focused input/textarea selection or the current
contenteditable range while dispatching an input event. Use it for command
palettes, native editor chrome, tests, and AI agents that need to type into a
hosted editor without handwritten page JavaScript. It does not bypass browser
focus, disabled/read-only fields, or page-level validation.
`focus_selector(...)` and `click_selector(...)` return
`Result<bool, SharedString>` after querying the first matching element in the
top document, scrolling it into view, and calling the browser's normal
`focus(...)` or `click()` method. Use them with `insert_text(...)` for simple
agent/test flows such as focus a hosted search box, type, then click submit.
`add_class(...)`, `remove_class(...)`, and `toggle_class(...)` return
`Result<bool, SharedString>` after mutating `classList` on the first matching
top-document element. `set_attribute(...)` / `remove_attribute(...)` and
`set_style_property(...)` / `remove_style_property(...)` do the same for DOM
attributes and inline CSS properties. Use these for app-owned hosted widgets,
third-party embeds with stable selectors, visual test setup, and AI-agent
customization without writing raw JavaScript. They do not pierce cross-origin
frames or shadow roots, and page script/browser policy can still reinterpret or
override sensitive attributes and styles.
`set_form_value(...)` returns `Result<bool, SharedString>` after setting the
first matching input, textarea, select, checkbox, radio, or contenteditable
element and dispatching normal `input` and `change` events. Use it for hosted
settings/auth/checkout widgets where the app owns the selector and wants a
small Rust-side fill helper. `submit_form(...)` returns
`Result<bool, SharedString>` after finding a matching form or nearest ancestor
form from a selected control, then using `requestSubmit()` where available so
browser validation and submit handlers run. It falls back to a cancelable submit
event and `form.submit()` for older engines. `reset_form(...)` returns
`Result<bool, SharedString>` after finding a matching form or nearest ancestor
form and calling normal `form.reset()` so hosted default values and reset
listeners run. These are convenience helpers, not a full Playwright-style
automation engine: they do not pierce cross-origin frames, shadow roots, or
browser permission prompts. `selected_text(...)`
returns `Result<SharedString, SharedString>` for the current browser document or
focused input/textarea selection. `selected_html(...)` serializes cloned
document selection ranges as HTML and returns escaped selected text for focused
input/textarea controls. `document_html(...)` returns
`document.documentElement.outerHTML` for inspectors, export flows, bug reports,
and AI-agent page understanding; cross-origin frames remain browser-owned and
are not expanded into that string. `document_snapshot(...)` returns structured
same-document metadata and page-understanding data: URL, title, ready state,
language, direction, truncated visible text, total text length, headings, links,
images, and forms. Use it for diagnostics, browser inspectors, tests, and
AI-agent planning when raw HTML is too noisy. `element_snapshot(...)` returns
`Result<Option<WebViewElementSnapshot>, SharedString>` for the first matching
top-document element. It captures tag name, id, classes, normalized text,
form-control value/checked/disabled state, editable/hidden flags, nearest link
or media/image source, viewport rectangle, attributes, and a few computed style
signals. Use it before selector mutations, context-menu actions, visual tests,
and AI-agent plans that need to inspect one hosted control without raw
JavaScript. It returns `Ok(None)` for no match and does not inspect
cross-origin frames or shadow roots. `capture_dom_image(...)` returns
`Result<Option<SharedString>, SharedString>` with an SVG data URL for a selected
same-document element. It clones the element, inlines computed styles, mirrors
common form values, and wraps the clone in an SVG `foreignObject`; use
`WebViewDomImageCaptureOptions` to set width, height, background, and maximum
pixel area. This is useful for app-owned widget thumbnails, receipts, previews,
visual test artifacts, and AI-agent page previews. It is not a native pixel
screenshot or full Electron `capturePage()` equivalent: it does not pierce
cross-origin frames or shadow roots, and browser media, canvas, WebGL, plugin
surfaces, external fonts, and remote images may not serialize with visual
fidelity. Use `capture_media_frame(...)` for the current frame of browser
`<video>` elements. Use `.edit_command(...)` when a builder or agent wants to route
through the generic command enum instead of the named helpers.

For native app windows, use a checked app-window capture request instead of a
WebView DOM image:

```rust
let capture = cx.app_window_capture_request_checked(
    AppWindowCaptureRequest::focused_window("Capture visual regression evidence.")
        .png()
        .max_dimensions(1920, 1080)
        .max_pixels(2_073_600),
)?;
```

This is Kael's native `capturePage()`-style contract for tests, support
diagnostics, and AI agents. `AppWindowCaptureRequestBuilder` can target the
focused app window, a specific app window, or all visible app windows, and it
validates purpose text, PNG vs raw RGBA output, window chrome/cursor flags,
dimension and pixel limits, plus the rule that multi-window captures cannot
include one cursor. Gate backend use with `PlatformFeature::AppWindowCapture`.
Visible app-owned render snapshots do not require screen-capture permission;
requests that allow occluded/minimized OS-level capture expose
`Some(Capability::ScreenCapture)` from `required_capability()`.
Context-menu bridge callbacks receive `WebViewContextMenuEvent` with page
coordinates, selection text, nearest link/image/media sources, and editable
field metadata so native menus can enable actions such as Open Link, Copy
Image, Save Media, Paste, or Inspect without writing bespoke page scripts for
every embed.
Pointer bridge callbacks receive `WebViewPointerEvent` with page coordinates,
buttons, pointer type, target tag, nearest link/image/media sources, and
editable field metadata so native status bars, hover previews, click handling,
tests, and agents can inspect hosted content without preventing browser
defaults.
Runtime CSS insertion creates or replaces a named
`<style data-kael-style-key="...">` block; use app-owned keys such as
`"checkout-theme"` or `"reader-overrides"` so the same block can be updated or
removed later.
Title callbacks receive `Result<SharedString, SharedString>` and read
`document.title` on demand; use `.on_document_title_changed(...)` when you need
continuous synchronization.
Favicon callbacks receive `Result<Vec<SharedString>, SharedString>` from
`favicons(...)` or `WebViewFaviconEvent { urls }` from `.on_favicon_changed(...)`;
use these URLs for native tab icons, breadcrumbs, history rows, and hosted-app
switchers.
User-agent callbacks receive `Result<SharedString, SharedString>` and read
`navigator.userAgent`; use `WebViewOptions::user_agent(...)` or
`webview(...).user_agent(...)` when you need to set the initial user agent.
Loading-state callbacks receive `Result<bool, SharedString>` and use
`document.readyState !== "complete"` for app-owned spinners and route guards;
use `.on_page_load(...)` when you need event-driven lifecycle updates.
Back-state callbacks receive `Result<bool, SharedString>` and use
`history.length > 1` for portable Back button gating. The browser History API
does not expose a reliable forward-stack read. `can_go_forward(...)` therefore
reads `window.__kaelNavigationState.canGoForward` or
`window.kaelNavigationState.canGoForward` when present and otherwise returns
`false` conservatively. Use `.navigation_state_bridge()` for app-owned
same-document WebView navigation; it tracks `pushState`, `replaceState`, and
`popstate` entries created after injection and publishes that marker for native
Forward buttons. Keep backend-native forward stack reads on the hardening
roadmap for cross-page and third-party navigation.
Location bridge callbacks receive
`WebViewLocationEvent { url, title, ready_state, can_go_back, can_go_forward }`
from `.on_location_changed(...)` and are the event-driven route-sync path for
hosted SPAs, native breadcrumbs, tab labels, and AI-agent state tracking.
Lifecycle bridge callbacks receive
`WebViewLifecycleEvent { event, visibility_state, hidden, has_focus, fullscreen, persisted }`
from `.on_lifecycle_event(...)` and are the event-driven browser-page path for
pausing hosted work, marking tabs inactive, reacting to browser fullscreen, or
letting agents know whether embedded content is focused or visible.
Scroll bridge callbacks receive
`WebViewScrollEvent { event, x, y, max_x, max_y, viewport_width, viewport_height, scroll_width, scroll_height, progress_x, progress_y }`
from `.on_scroll_event(...)` and are the event-driven browser-page path for
reader progress, sticky native chrome, scroll restoration, and agent viewport
inspection. For on-demand viewport work, keep the controller and call
`viewport_snapshot(window, callback)`, `scroll_to(window, x, y, callback)`,
`scroll_by(window, dx, dy, callback)`, or
`scroll_selector_into_view(window, selector, callback)`. These helpers return
the same `WebViewScrollEvent` shape after reading or moving the top document.
`scroll_selector_into_view(...)` returns `Ok(None)` when no top-document element
matches. Cross-origin frames and shadow roots remain browser-owned.
Selection bridge callbacks receive
`WebViewSelectionEvent { event, selected_text, selected_html, collapsed, editable, input_kind }`
from `.on_selection_event(...)` and are the event-driven browser-page path for
native Edit menu enablement, floating formatting bars, rich-editor integrations,
and agent selection inspection.
The browser-media helpers operate on current
`document.querySelectorAll("audio,video")` elements. `play_media(window)` calls
each element's `play()` and swallows rejected promises, so browser autoplay and
user-gesture policies still apply; it is a convenient app-owned command, not a
cross-browser autoplay bypass. `set_media_volume(...)` clamps to `0.0..=1.0`,
`set_media_playback_rate(...)` sends the requested non-negative rate, and
`seek_media_secs(...)` clamps negative or non-finite input to `0.0`.
`media_command(...)` takes `WebViewMediaCommand` when native chrome, tests, or
agents need to play, pause, toggle, stop, mute, change volume, change playback
rate, or seek one matching media element or descendant instead of every media
element on the page.
`set_media_source(...)` returns `Result<bool, SharedString>` after finding a
matching `<audio>`, `<video>`, or nested `<source>`, assigning the new browser
media `src`, and calling `load()` so normal metadata, buffering, and media
events run. `set_media_options(...)` applies
`WebViewMediaElementOptions` to a matching `<audio>` or `<video>` so native
chrome and agents can toggle browser controls, loop, autoplay, muted,
playsinline, poster, preload, controlslist, and picture-in-picture disablement
without custom page JavaScript. `capture_media_frame(...)` returns
`Result<Option<SharedString>, SharedString>` with a browser canvas data URL for
the current frame of a matching `<video>`; it returns `Ok(None)` when no frame
is drawable or browser CORS/tainted-canvas rules block capture. Use
`WebViewMediaFrameCaptureOptions` to request size, MIME type, and quality.
`add_media_text_track(...)` appends a real browser `<track>` from
`WebViewMediaTextTrackOptions`, usually a WebVTT URL or data URL, so the
embedded browser owns cue loading/parsing and the resulting track appears in
`media_state(...)`. `remove_media_text_track(...)` removes
matching `<track>` children from a hosted media element by track id, label,
language, kind, src, or zero-based index so apps can swap subtitle sets without
reloading the WebView.
`select_media_text_track(...)` matches text-track id, label, language, or
zero-based index across current media elements and sets matching tracks to
`showing` while disabling the rest; `disable_media_text_tracks(...)` disables
all browser text tracks. Browser
fullscreen and picture-in-picture helpers call the standard element/document
APIs when present and swallow rejected promises; browser support, page
attributes, embedding policy, permissions, and user-gesture requirements still
apply.
`media_state(window, callback)` returns
`Result<Vec<WebViewMediaElementState>, SharedString>` with tag name, DOM id,
source, paused/ended/muted/seeking flags, volume, playback rate, current time,
optional duration, ready/network state, fullscreen and picture-in-picture
booleans, buffered ranges, browser text-track metadata, and active cue text for
native controls that drive browser-hosted players. `WebViewMediaEvent` uses the
same `WebViewMediaElementState` shape for event-driven updates.
Use browsing-data cleanup for logout, account switching, demo reset buttons,
and test isolation; with a persistent
`.storage_key(...)` / `WebViewOptions::auth_flow(...)`, the cleanup applies to
that WebView profile. Linux/Windows Wry-backed WebViews and Kael's custom
macOS WKWebView backend honor these commands through their profile cookie
stores today.

For lifecycle coordination, use `.on_page_load(...)` to receive
`WebViewPageLoadEvent::{Started, Finished}` plus the URL. This covers common
Electron `did-start-loading` / `did-finish-load` style flows such as showing
spinners, deferring host messages until the page is ready, or observing hosted
auth redirects. Wry-backed Linux/Windows WebViews and Kael's custom macOS
WKWebView backend honor this handler today.

```js
const result = await window.kael.invoke("pick-video", { accept: ["video/*"] });
video.src = result.path;
window.kael.post("checkout-complete", { id: checkoutId });
```

Use the named option presets when they match the intent:

- `WebViewOptions::auth_flow(storage_key)` for OAuth, SSO, and account pages
  that need persistent cookies/session storage.
- `WebViewOptions::embedded_widget()` for payments, maps, docs, customer
  widgets, and other ephemeral third-party surfaces.
- `WebViewOptions::web_graphics()` for WebGL/WebGPU/canvas islands that should
  fill their element without browser scroll chrome.
- Add `.devtools()` to any option bundle while developing WebView islands.
- Add `.console_bridge(...)` or `.on_console_message(...)` when native
  diagnostics, tests, or agents should receive hosted-page console output.
- Add `.zoom_hotkeys()` / `.zoom_hotkeys_enabled(...)` when browser content
  should own zoom keyboard shortcuts or gestures.
- Add `.keyboard_event_bridge(...)` or `.on_keyboard_event(...)` when native
  chrome should observe hosted keydown/keyup/beforeinput activity.
- Add `.location_bridge(...)` or `.on_location_changed(...)` when native tabs,
  breadcrumbs, Back/Forward chrome, or agents should observe hosted SPA route
  changes.
- Add `.lifecycle_bridge(...)` or `.on_lifecycle_event(...)` when native chrome,
  resource throttling, tests, or agents should observe hosted focus, visibility,
  page show/hide, and browser fullscreen changes.
- Add `.scroll_bridge(...)` or `.on_scroll_event(...)` when native progress,
  sticky chrome, scroll restoration, tests, or agents should observe hosted
  scroll and viewport state.
- Keep a controller and call `viewport_snapshot(...)`, `scroll_to(...)`,
  `scroll_by(...)`, or `scroll_selector_into_view(...)` when native chrome,
  tests, or agents need to move or inspect the hosted top-document viewport.
- Keep a controller and call `add_class(...)`, `remove_class(...)`,
  `toggle_class(...)`, `set_attribute(...)`, `remove_attribute(...)`,
  `set_style_property(...)`, or `remove_style_property(...)` when hosted
  widgets need selector-scoped DOM customization without bespoke JavaScript.
- Call `element_snapshot(...)` first when native chrome, tests, or agents need
  to inspect one hosted element before deciding whether to focus, click, fill,
  style, or save related content.
- Add `.selection_bridge(...)` or `.on_selection_event(...)` when native edit
  menus, formatting chrome, tests, or agents should observe hosted selection
  state.
- Add `.media_autoplay()` / `.media_autoplay_enabled(...)` for browser-media
  islands that need an explicit autoplay policy.
- Add `.context_menu_bridge(...)` or `.on_context_menu(...)` when native chrome
  should own right-click / secondary-click menus for hosted WebView content.
- Add `.pointer_bridge(...)` or `.on_pointer_event(...)` when native status
  bars, hover previews, click handling, tests, or agents need lightweight
  link/image/media/editable context for hosted content.
- Add `.form_bridge(...)` or `.on_form_event(...)` when native validation,
  progress chrome, tests, or agents should observe hosted submit, reset, change,
  and input activity without bespoke page JavaScript.
- Add `.file_input_bridge(...)` or `.on_file_input_event(...)` when native
  upload chrome, tests, or agents should observe browser file-input selections
  with file names, sizes, MIME types, and last-modified timestamps.
- Add `.resource_bridge(...)` or `.on_resource_event(...)` when native
  diagnostics, loading UI, tests, or agents should observe hosted subresource
  timing plus element load/error activity without request interception.
- Add `.network_bridge(...)` or `.on_network_event(...)` when native
  diagnostics, loading UI, tests, or agents should observe hosted fetch/XHR
  outcomes without opening devtools.
- Add `.dialog_bridge(...)` or `.on_dialog_event(...)` when native diagnostics,
  tests, or agents should observe hosted `alert`, `confirm`, `prompt`, and
  `beforeunload` activity while preserving browser behavior.
- Add `.clipboard_event_bridge(...)` or `.on_clipboard_event(...)` when native
  editor chrome, tests, or agents should observe hosted copy/cut/paste events
  and browser-exposed clipboard data.
- Add `.permission_bridge(...)` or `.on_permission_request(...)` when native
  app policy should preflight hosted camera, microphone, display-capture,
  geolocation, or notification requests before browser permission prompts
  continue.
- Add `.storage_bridge(...)` or `.on_storage_event(...)` when native account
  chrome, settings sync, tests, or agents should observe hosted local/session
  storage changes without polling JavaScript.
- Add `.navigation_state_bridge()` when native chrome should enable or disable
  a Forward button for app-owned same-document WebView navigation.
- Add `.focused()` / `.focused_enabled(...)` when a WebView island should take
  keyboard focus as soon as it is created.
- Add `.clipboard_access()` / `.clipboard_access_enabled(...)` for hosted rich
  editors and browser widgets that need JavaScript clipboard APIs.
- Add `.javascript_disabled()` / `.javascript_disabled_enabled(...)` for
  untrusted static docs and previews that should not run page scripts.
- Add `.general_autofill_enabled(...)` / `.general_autofill_disabled()` for
  Windows/WebView2 general form suggestions.
- Add `.request_headers(...)` / `.clear_request_headers()` when the first
  navigation needs Electron-style extra request headers.
- Add `.html(...)` / `.clear_html()` when an option bundle should provide an
  initial raw HTML document instead of a URL.
- Add `.deny_new_windows()`, `.open_new_windows_in_current_webview()`,
  `.allow_new_windows()`, or `.on_new_window(...)` for popup and target-blank
  behavior.
- Add `.allow_downloads()`, `.deny_downloads()`, `.on_download_started(...)`,
  and `.on_download_completed(...)` when browser content can download files.
  Pair this with `WebViewController::trigger_download(...)` for native "Save
  link/image/media" commands sourced from context-menu or pointer bridge URLs.
- Add `.on_drag_drop(...)` or `.block_drag_drop()` when browser content can
  receive local file drops and the app needs to preserve or block browser
  defaults explicitly.
- Add `.on_document_title_changed(...)` when hosted content should drive native
  titles, tabs, or breadcrumbs.
- Add `.favicon_bridge(...)` or `.on_favicon_changed(...)` when hosted content
  should drive native tab icons, breadcrumbs, history rows, or app switchers.
- Add `.on_page_load(...)` when hosted content needs native loading state,
  ready coordination, or auth-flow progress.

All presets can be refined with `.storage_key(...)`, `.user_agent(...)`,
`.inject_css(...)`, `.inject_javascript(...)`, `.bridge_script()`,
`.devtools()`, `.console_bridge(...)`, `.on_console_message(...)`,
`.zoom_hotkeys()`, `.zoom_hotkeys_enabled(...)`,
`.find_result_bridge(...)`, `.on_find_result(...)`,
`.keyboard_event_bridge(...)`, `.on_keyboard_event(...)`,
`.location_bridge(...)`, `.on_location_changed(...)`,
`.lifecycle_bridge(...)`, `.on_lifecycle_event(...)`,
`.scroll_bridge(...)`, `.on_scroll_event(...)`,
`.selection_bridge(...)`, `.on_selection_event(...)`,
`.media_autoplay()`, `.media_autoplay_enabled(...)`, `.media_event_bridge(...)`,
`.on_media_event(...)`, `.context_menu_bridge(...)`, `.on_context_menu(...)`,
`.pointer_bridge(...)`, `.on_pointer_event(...)`,
`.form_bridge(...)`, `.on_form_event(...)`,
`.file_input_bridge(...)`, `.on_file_input_event(...)`,
`.resource_bridge(...)`, `.on_resource_event(...)`,
`.network_bridge(...)`, `.on_network_event(...)`,
`.dialog_bridge(...)`, `.on_dialog_event(...)`,
`.clipboard_event_bridge(...)`, `.on_clipboard_event(...)`,
`.permission_bridge(...)`, `.on_permission_request(...)`,
`.storage_bridge(...)`, `.on_storage_event(...)`,
`.navigation_state_bridge()`,
`.html(...)`, `.clear_html(...)`, `.focused()`, `.focused_enabled(...)`, `.clipboard_access()`,
`.clipboard_access_enabled(...)`, `.javascript_disabled()`,
`.javascript_disabled_enabled(...)`, `.general_autofill_enabled(...)`,
`.general_autofill_disabled()`, `.request_headers(...)`,
`.clear_request_headers()`, `.deny_new_windows()`,
`.open_new_windows_in_current_webview()`,
`.allow_new_windows()`, `.allow_downloads()`, `.deny_downloads()`,
`.on_download_started(...)`, `.on_download_completed(...)`,
`.on_drag_drop(...)`, `.block_drag_drop()`, `.on_message(...)`,
`.on_bridge_message(...)`, `.favicon_bridge(...)`, `.on_favicon_changed(...)`,
`.on_document_title_changed(...)`,
`.on_page_load(...)`, `.on_navigate(...)`, `.on_new_window(...)`, and
`.allow_navigation_schemes(...)`. For tiny one-off embeds, the existing fluent
methods on `webview(id, url)` remain available.

Recommended rule:

- Use native Kael for app chrome, navigation, panes, toolbars, forms, lists, and
  performance-sensitive surfaces.
- Use WebView islands when the value comes from web compatibility itself.
- Keep WebView boundaries explicit and message-based so apps do not become a
  hidden browser app by accident.

## Platform APIs need builder-shaped affordances

Electron feels productive partly because common desktop capabilities are one
obvious call away. Kael already has many of the native backends, but the public
surface should increasingly expose builder-shaped APIs that are easy for humans
and AI agents to compose without remembering raw arrays or callback plumbing.

Notifications are the pattern:

```rust
cx.show_notification_checked("Build Complete", "All tests passed")?;

cx.show_desktop_notification(
    NotificationBuilder::new("Build Complete", "All tests passed")
)?;

cx.show_desktop_notification_with_actions(
    NotificationBuilder::new("Update Available", "Version 2.0 is ready to install")
        .open_action("Install Now")
        .dismiss_action("Remind Later"),
    |action_id| {
        println!("clicked: {action_id}");
    },
)?;
```

This does not replace lower-level calls such as
`show_notification(...)` or `show_notification_with_actions(...)`; it gives
builders a safer default path. Use `show_notification_checked(...)` for plain
notifications and `.action(id, label)` for custom routing. Builder validation
rejects duplicate action IDs before callbacks become ambiguous.
Tray menus follow the same direction:

```rust
cx.set_tray_menu_checked(
    TrayMenuBuilder::new()
        .action("Show Window", "show")
        .separator()
        .toggle("Pause Sync", false, "pause-sync")
        .submenu(
            "Status",
            TrayMenuBuilder::new()
                .toggle("Available", true, "available")
                .action("Set Away", "away"),
        )
        .action("Quit", "quit"),
)?;
cx.set_tray_tooltip_checked(TrayTooltipBuilder::status("Sync complete"))?;
```

Use `TrayTooltipBuilder::status(...)`, `text(...)`, or `clear()` for generated
tray/background app status. The checked path rejects empty tooltips, padded text,
control characters, and text longer than 256 characters; raw
`cx.set_tray_tooltip(...)` remains available for already-validated platform
behavior.

Context menus use the same native item model with a context-specific builder:

```rust
cx.show_context_menu_checked(
    mouse_position,
    NativeContextMenuBuilder::new()
        .action("Open", "open")
        .separator()
        .submenu(
            "Sort",
            NativeContextMenuBuilder::new()
                .action("By Name", "sort-name")
                .toggle("Descending", false, "sort-desc"),
        ),
    |action_id, _cx| {
        println!("context menu action: {action_id}");
    },
)?;
```

Both checked paths validate empty labels, empty action IDs, empty submenus, and
duplicate action IDs across nested menu trees before the OS menu is installed.

Apply this pattern to deep-link setup and other remaining platform surfaces:
keep the native platform capability explicit, but make the 80% path one fluent
object with validation, docs, and capability-report guidance.

Clipboard text now follows that rule too:

```rust
cx.write_clipboard_text_checked("Copied from Kael")?;

if let Some(text) = cx.read_clipboard_text()? {
    println!("clipboard: {text}");
}
```

For richer paste/copy workflows, use the validated clipboard builder instead of
manually constructing entry arrays:

```rust
cx.write_clipboard_item(
    ClipboardItem::builder()
        .try_text_with_json_metadata(
            "formatted text",
            serde_json::json!({ "source": "my_app" }),
        )?
        .image_ref(&preview_image),
)?;

if let Some(item) = cx.read_from_clipboard()? {
    if item.has_text() {
        println!("text: {:?}", item.text());
    }
    if let Some(image) = item.first_image() {
        println!("image format: {:?}", image.format());
    }
}
```

Raw `ClipboardItem` constructors remain available, but the builder path gives
agents a safer way to combine text, JSON metadata, and images. The older
`write_clipboard_text(...)` convenience method remains available for
already-validated text, while generated code should prefer
`write_clipboard_text_checked(...)`.

Share sheets follow the same checked-builder shape when apps need Electron-like
"share/export this" handoff to the operating system:

```rust
let result = cx
    .show_share_sheet(
        ShareSheet::builder()
            .subject("Build report")
            .text("All checks passed")
            .url("https://example.com/report")
            .file(report_path)
            .exclude(ShareType::Social),
    )
    .await?;
```

`ShareItem::{text,url,file,files,image}` and
`ShareSheet::{text,url,file,files}` cover one-line cases, while
`ShareSheetBuilder` handles export bundles. The checked path validates non-empty
payloads, URL schemes, image data, and file existence before invoking the
platform backend; `cx.share_support()` reports available destination families.

Operating-system file drops should also use the typed drag/drop path instead
of platform-event bookkeeping:

```rust
let filter = FileDropFilter::video().max_files(1);

div()
    .id("video-drop-zone")
    .can_drop_external(filter.clone())
    .on_external_drop(move |data, _window, _cx| {
        if let Some(paths) = data.accepted_paths_by(&filter) {
            for path in paths {
                // Open or import the dropped file.
            }
        }
        for url in data.urls() {
            // Import or embed the dropped URL.
        }
    });
```

This mirrors the Electron mental model of dropping files into an editor,
uploader, or media player, while keeping the native typed drag/drop system.
Use `FileDropFilter::images()`, `.audio()`, `.video()`, `.media()`, or
`.single_file()` for common drop zones before falling back to custom
`.extensions([...])` filters. `can_drop_external(filter)` applies the filter to
file paths and still accepts text/URL-only payloads.

After the user drops files, convert accepted paths into an app-owned intent
before importing or opening them:

```rust
let intent = cx.file_drop_intent_checked(
    FileDropIntentBuilder::media_source()
        .paths(paths)
        .max_paths(4)
        .canonicalize_paths(),
)?;

for path in intent.paths() {
    open_media(path)?;
}
```

`FileDropIntentBuilder` validates the semantic purpose, max path count,
file-vs-directory policy, extension allowlists, optional existence,
canonicalization, and deduplication. Use it for Electron-style drag-to-open,
project import, folder import, media-player drops, and AI-agent file intake.

For drag-out/export workflows, use a checked file export drag descriptor rather
than generating a temporary WebView download:

```rust
let export = cx.file_export_drag_checked(
    FileExportDragIntentBuilder::generated_files("Drag rendered poster.")
        .virtual_file_with_mime("poster.png", "image/png", poster_bytes)
        .max_virtual_file_bytes(32 * 1024 * 1024),
)?;
```

`FileExportDragIntentBuilder` covers existing file paths and generated virtual
files/file promises. It validates purpose text, item limits, safe file names,
MIME type shape, non-empty generated bytes, byte limits, and optional existence
for existing paths. Existing-path exports declare a
`Capability::FilesystemRead { scope: PathScope::UserSelected }` requirement;
virtual/generated exports do not need filesystem access. Gate the platform
backend with `PlatformFeature::FileExportDrag`. This gives design tools, media
editors, report builders, and AI artifact apps an Electron-style drag-to-desktop
story without depending on browser download behavior.

When the same accepted paths need routing, classify them once:

```rust
let intake = cx.file_intake_plan_checked(
    FileIntakePlanBuilder::new()
        .paths(intent.paths().iter().cloned())
        .canonicalize_paths(),
)?;

for project in intake.paths_of_kind(FileIntakeKind::Project) {
    open_project(project)?;
}
```

`FileIntakePlanBuilder` covers the common extension-based branch that Electron
apps often hand-roll after file dialogs, drops, recent documents, or
file-opening events: directories, project/workspace files, images, audio, video,
PDFs, text, structured data, archives, and unknowns. Add `.reject_unknown()` for
strict importers.

For document apps, declare the file types the app owns as checked metadata:

```rust
let associations = cx.file_associations_checked(
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
                .editor(),
        ),
)?;
```

This is Kael's bridge for Electron packaging document-type metadata. It gives
bundlers, installers, docs, and AI agents a validated declaration of supported
extensions and MIME types, while runtime opens still flow through open requests,
recent documents, file dialogs, drops, and file intake. Extensions are
normalized, MIME types are validated, and duplicate claims are rejected before a
generated app ships contradictory metadata.

For Electron `app.getFileIcon(...)` parity in file explorers, recent files,
upload pickers, and project launchers, use a checked file-icon request before
calling a platform icon backend:

```rust
let icon = cx.file_icon_request_checked(
    FileIconRequestBuilder::new(project_path)
        .large()
        .require_existing_path(),
)?;
```

Use `.small()`, `.normal()`, `.large()`, or `.custom_size_px(size)` to request
the desired native icon size. Missing planned paths such as `"Draft.kaelproj"`
are allowed only when generic extension fallback is enabled and an extension
hint is present; concrete user paths can opt into `.require_existing_path()` and
`.canonicalize_path()`.

For Electron `app.setAsDefaultProtocolClient(...)` and default document-handler
intent, build a checked plan before any OS registration work:

```rust
let defaults = cx.default_handler_plan_checked(
    DefaultHandlerPlanBuilder::new("com.example.kael-studio")
        .app_name("Kael Studio")
        .schemes(["kael", "kael-auth"])
        .file_association(
            FileAssociationBuilder::new("Kael Project")
                .extension("kaelproj")
                .mime_type("application/x-kael-project")
                .editor(),
        )
        .current_user_scope(),
)?;
```

`DefaultHandlerPlanBuilder::from_package_manifest(&manifest)` seeds the same
runtime/setup intent from checked package metadata. The plan validates app
identity, schemes, document claims, duplicate claims, scope, and user-facing
prompt text, but does not mutate OS defaults by itself. Hand it to installer
code, first-run setup, or platform-specific registry/default-app glue.

When a generator needs the broader Electron-builder style package contract,
compose identity, schemes, and document types into one checked manifest:

```rust
let manifest = cx.package_manifest_checked(
    AppPackageManifestBuilder::new(
        AppMetadataBuilder::new("Kael Studio")
            .identifier("com.example.kael-studio")
            .version(env!("CARGO_PKG_VERSION")),
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
    .icons(
        AppIconSetBuilder::new()
            .icon(AppIconAssetBuilder::app("assets/app.icns"))
            .icon(AppIconAssetBuilder::tray("assets/tray.svg").template())
            .icon(AppIconAssetBuilder::document("assets/document.png").size_px(128)),
    )
    .privacy_permissions(
        AppPrivacyManifestBuilder::new()
            .permission(AppPrivacyPermissionBuilder::camera(
                "Camera access records video notes.",
            ))
            .permission(AppPrivacyPermissionBuilder::microphone(
                "Microphone access records narration.",
            )),
    ),
)?;

let readiness = manifest.readiness_report();
if !readiness.is_ready() {
    return Err(anyhow::anyhow!(readiness.summary()));
}

let dist = cx.distribution_plan_checked(
    AppDistributionPlanBuilder::new("/tmp/kael-dist")
        .target(AppDistributionTargetBuilder::dmg())
        .target(AppDistributionTargetBuilder::msi().channel("stable"))
        .target(AppDistributionTargetBuilder::appimage()),
)?;

let signing = cx.signing_plan_checked(
    AppSigningPlanBuilder::new()
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
        .target(AppSigningTargetBuilder::linux_package("kael-release-key")),
)?;
```

`AppPackageManifestBuilder` exports platform-shaped declarations for macOS
bundle URL/document entries, Linux desktop MIME types, and Windows installer
file associations. It also carries checked app, tray, document, and installer
icon declarations so Electron-style `nativeImage`/packaging icon metadata has a
typed home before platform conversion. Privacy declarations cover the
packaging-time side of Electron permission work: camera, microphone, screen
capture, location, notifications, filesystem, network, USB, HID, serial-port,
and Bluetooth intent get validated user-facing reasons and known macOS
usage-description entries where applicable.
Runtime access still goes through Kael's capability broker. That gives
packaging tools and AI agents a stable typed handoff without smuggling installer
metadata through ad hoc strings.

For native geolocation, use a checked request descriptor instead of relying on
browser geolocation from a hidden WebView:

```rust
let location = cx.location_request_checked(
    LocationRequestBuilder::new("Show nearby workspaces.")
        .balanced()
        .timeout(Duration::from_secs(10))
        .maximum_age(Duration::from_secs(300)),
)?;
```

`LocationRequestBuilder` validates purpose text, timeout, cached-location age,
and background/accuracy combinations. Gate execution with
`PlatformFeature::Geolocation`, request `Capability::Location` through the
permission broker, and include `location.privacy_permission()` in packaging
metadata. WebView geolocation permission bridges remain useful for hosted
browser content; app-owned native features should use the native descriptor.

For WebUSB, WebHID, Web Serial, and Web Bluetooth parity, Kael exposes checked
native request descriptors so hardware access is not treated as a WebView-only
feature:

```rust
let device = cx.device_access_request_checked(
    DeviceAccessRequest::hid("Read shortcut events from the editing console.")
        .vendor_product(0x1234, 0xabcd),
)?;
```

Use `DeviceAccessRequest::usb(...)`, `hid(...)`, `serial(...)`, or
`bluetooth(...)` to declare the app-owned device family, then add the relevant
filter: USB/HID vendor/product ids, serial `port_name_hint(...)`, or Bluetooth
`service_uuid(...)`. Checked builders reject empty/padded/control-character
reasons, zero or longer-than-120-second timeouts, product ids without vendor
ids, invalid Bluetooth UUIDs, and filters that belong to another device family.
Gate execution with `PlatformFeature::UsbDevices`, `HidDevices`, `SerialPorts`,
or `BluetoothDevices`, request `Capability::UsbDevice`, `HidDevice`,
`SerialPort`, or `Bluetooth`, and pass `device.privacy_permission()` into the
package manifest. The current bridge intentionally lands descriptor,
capability, and packaging contracts first; per-platform discovery/IO backends
can build on that without hiding privileged hardware access in browser
JavaScript.

Run `manifest.readiness_report()` or
`cx.package_readiness_checked(AppPackageReadinessBuilder::new(manifest))` before
emitting installer files. The readiness report catches blocking gaps such as a
missing app version or primary icon, and non-blocking packaging warnings such as
document associations without document icons, extension-only file associations,
or privacy declarations that have no known platform usage-description export.
Use `AppDistributionPlanBuilder` for the Electron-builder target-list part of
the flow: declare `dmg`, `mac-zip`, `msi`, `nsis`, `appimage`, `deb`, `rpm`, or
`tar-gz` targets, optional release channels, and an absolute artifact output
directory. The plan validates target shape and derives artifact paths from the
checked manifest; platform bundlers still own signing, notarization, and archive
creation.
Pair it with `AppSigningPlanBuilder` when release scripts need Electron-builder
style signing/notarization intent. The checked plan rejects empty signing sets,
duplicate platforms, invalid identity/team labels, non-macOS notarization or
hardened-runtime flags, and notarization without a macOS signing identity. Call
`signing.covers_distribution_plan(&dist)` before release to catch an unsigned
target before a platform bundler starts.

For WebView bridges or custom platform integrations that need the browser
`DataTransfer` shape, normalize to `ExternalDropData`: it can carry file paths,
plain text, and URLs together, and still exposes `accepted_paths_by(...)` for
the file portion. Use `ExternalDropData::from_drag_value(value)` to normalize an
active drag payload, `ExternalDropData::from_uri_list(...)` for `text/uri-list`
payloads, and `ExternalDropData::from_plain_text(...)` for plain-text drops that
may contain URLs. File-only OS drops still emit
`ExternalPaths` for compatibility; macOS and Windows native text/URL drops and
Linux URI-list drops emit `ExternalDropData` when there is non-file data to
preserve.

Secure credentials should have a keychain-shaped 80% path for auth-heavy apps:

```rust
cx.write_secure_credential(
    CredentialBuilder::new("https://api.example.com")
        .username("ada")
        .password(refresh_token),
)?.await?;

if let Some(credential) = cx
    .read_secure_credential("https://api.example.com")
    .await?
{
    println!("credential account: {}", credential.username());
}
```

The wrapper validates service, username, and secret before delegating to the
platform keychain / credential manager, including rejecting accidentally padded
service or username strings. Raw `write_credentials(...)`,
`read_credentials(...)`, and `delete_credentials(...)` remain available for
lower-level integrations.

Permissions now have a grouped startup path for apps that need Electron-style
access to media devices or accessibility automation:

```rust
let permissions = cx.request_permissions(
    PermissionRequestBuilder::new()
        .accessibility()
        .media_devices(),
)?;

if permissions.has_blocking_denial() {
    if let Some(summary) = permissions.blocking_denial_summary() {
        eprintln!("permissions blocked: {summary}");
    }
    for denial in permissions.blocking_denials() {
        // Use denial.key to route settings guidance or choose a fallback.
    }
}
```

The snapshot reports the current OS status before any prompt is launched.
Microphone and camera prompts can attach callbacks with
`.microphone_with_callback(...)` and `.camera_with_callback(...)`; the raw
single-permission methods remain available for just-in-time prompts.

Power management should also be builder-shaped for media, presentation, capture,
and background-task apps:

```rust
let blocker = cx.start_power_save_blocker_checked(
    PowerSaveBlockerBuilder::prevent_display_sleep()
        .reason("video playback"),
)?;

// Later, when playback or capture ends:
if let Some(blocker) = blocker {
    blocker.stop(cx);
}
```

The lower-level `start_power_save_blocker(PowerSaveBlockerKind::...)` and
`start_power_save_blocker_with(...)` remain available, but the checked path
validates generated reasons and the typed handle keeps the platform ID, kind,
and reason together so generated apps are less likely to leak a blocker after
playback ends.

Adaptive power and accessibility preferences should be monitored through one
runtime snapshot:

```rust
let monitor = cx.watch_system_power_checked(
    SystemPowerMonitorBuilder::new()
        .on_power_mode_changed(|snapshot, _cx| {
            if snapshot.should_reduce_work() {
                // Lower polling, effects, or render quality.
            }
        })
        .on_suspend(|_snapshot, _cx| {
            // Save state.
        })
        .on_resume(|_snapshot, _cx| {
            // Refresh stale data.
        }),
)?;

if monitor.initially_should_reduce_work() {
    // Start in battery/accessibility friendly mode.
}
```

The raw `power_mode()`, `reduce_motion()`, `system_idle_time()`, and
`on_system_power_event(...)` APIs remain available for custom routers. Use
`watch_system_power(...)` for snapshot-only monitors without callbacks.

For Electron `nativeTheme`-style UI choices, use one native theme snapshot:

```rust
let theme = cx.native_theme_snapshot();
let panel_background = theme.choose(dark_panel, light_panel);

if theme.should_reduce_effects() {
    // Disable decorative blur, motion, or expensive effects.
}
```

`NativeThemeSnapshot` combines the current window appearance, reduce-motion
preference, and power mode, with helpers for dark/light/vibrant appearances and
a single `should_reduce_effects()` decision for generated UI.

For Electron-style "run this when the user has been idle" workflows, use
`SystemIdlePolicyBuilder` instead of repeating duration comparisons:

```rust
let idle = cx.system_idle_evaluation_checked(
    SystemIdlePolicyBuilder::minutes(5)
        .require_known_idle_time(),
)?;

if idle.is_idle() {
    // Run indexing, sync compaction, or expensive preview generation.
}
```

The checked policy rejects zero thresholds and contradictory unknown-idle
behavior. Platforms that cannot report idle time evaluate to `Unknown` by
default; opt into `.treat_unknown_as_idle()` only for work that is safe when idle
telemetry is unavailable.

Hardware media keys and OS media controls should route through the same media
controllers instead of forcing each app to hand-roll a match statement:

```rust
let video = VideoController::url(video_url);

MediaKeyBindingBuilder::new()
    .video(video.clone())
    .playlist(
        VideoPlaylist::new([
            MediaSource::url("https://cdn.example.com/intro.mp4"),
            MediaSource::url("https://cdn.example.com/lesson.mp4"),
            MediaSource::url("https://cdn.example.com/outro.mp4"),
        ])
        .repeat(true),
    )
    .install(cx);
```

The builder maps play, pause, play/pause, and stop to either `AudioHandle` or
`VideoController`; next/previous can use `VideoPlaylist` for simple
source-replacement queues, while `on_next_track(...)` and
`on_previous_track(...)` remain available for database-backed queues, analytics,
or custom preload logic. Raw `on_media_key_event(...)` is still available for
custom OS-control routing.

User attention should be just as explicit for background tasks, downloads,
calls, and failed long-running jobs:

```rust
let request = cx.request_user_attention_checked(
    UserAttentionBuilder::informational()
        .reason("download complete"),
)?;

// Cancel when the app becomes active or the condition is resolved.
request.cancel(cx);
```

`UserAttentionBuilder::critical()` maps to continuous or urgent platform
attention where the OS supports it. The checked path rejects empty reasons; the
raw `request_user_attention(...)`, `request_user_attention_with(...)`, and
`cancel_user_attention()` methods remain available for custom lifecycle code.

Network status should be a first-class runtime signal for sync, presence,
upload queues, and offline-first apps:

```rust
let monitor = cx.watch_network_status_checked(
    NetworkStatusMonitorBuilder::new()
        .on_offline(|cx| {
            // Pause sync and surface offline state.
        })
        .on_online(|cx| {
            // Resume queued work.
        }),
)?;

if !monitor.initially_online() {
    // Start in offline mode.
}
```

The raw `network_status()` and `on_network_status_change(...)` methods remain
available, and `watch_network_status(...)` remains useful for snapshot-only
monitors without callbacks.
available when an app needs its own router.

Screen, camera, microphone, and system-audio capture should start from the
app-wired manager when builders need Electron `desktopCapturer`-style workflows:

```rust
let manager = cx.capture_manager();
let sources = manager.sources(
    CaptureSourceQueryBuilder::screens_and_windows()
        .name_contains("Display")
        .limit(4),
)?;

let configs = manager.configs(
    CaptureConfigSetBuilder::screen_with_microphone()
        .video_frame_rate(30.0)
        .video_resolution(1920, 1080),
)?;

let mut pipeline = CapturePipeline::new();
for config in configs {
    let mut session = manager.create_session(&config)?;
    session.start(config, std::sync::Arc::new(|frame| {
        // Encode, preview, stream, or analyze captured frames.
    }))?;
    pipeline.add_session(session);
}
```

The helper registers platform-default backends and copies the app permission
broker/process ID into the capture manager, so screen/camera/microphone capture
still goes through the same capability model as the rest of the app. Use
`CaptureConfigBuilder::{screen, window, camera, microphone, system_audio}()`
for common capture kinds, `.device_name_contains(...)` for a stable user-facing
preference, or `.device_id(...)` after presenting `manager.devices(kind)` in a
custom picker. Use `CaptureConfigSetBuilder::screen_with_microphone()`,
`camera_with_microphone()`, or `screen_with_system_audio()` when an app needs a
coordinated screen share, camera call, or screen-recording setup without wiring
each source by hand. `CaptureConfig::new(...)`, `create_session(...)`, and
`create_session_with(...)` remain available for lower-level integrations.
Use `CaptureSourceQueryBuilder` for the Electron `desktopCapturer.getSources`
part of the flow before a capture session starts. It can query screens, windows,
or both, include unavailable sources for diagnostics, filter by display/window
name, and limit results for picker UI. The resulting `CaptureSourceCatalog`
keeps source metadata separate from capture constraints; choose a source for UI
or agent policy, then pass the selected ID into `CaptureConfigBuilder`.

Open/save dialogs now have explicit builders over the existing platform prompt
methods:

```rust
let paths = cx
    .show_open_dialog(
        OpenDialogBuilder::files()
            .image_files()
            .filter("Markdown", ["md", "markdown"])
            .prompt("Open"),
    )
    .await??;

let path = cx
    .show_save_dialog(
        SaveDialogBuilder::new(std::env::current_dir()?)
            .suggested_name("document")
            .text(),
    )
    .await??;
```

Open dialogs support Electron-style named extension filters through
`FileDialogFilter` presets such as `.image_files()`, `.audio_files()`,
`.video_files()`, `.pdf_files()`, `.text_files()`, or custom
`.filter("Documents", ["pdf", "docx"])` calls. The builder validates filter
names, extensions, and generated prompt labels before reaching platform code.
Save dialogs support default extension helpers with `.default_extension("pdf")`,
`.pdf()`, `.text()`, and `.json()`, appending the extension only when the
suggested name does not already include one. The builder rejects empty
directories, empty or padded suggested names, path separators in suggested
names, and malformed default extensions.

Message dialogs now have a builder path for alerts, confirmations, and errors:

```rust
let rx = cx.show_message_dialog(
    MessageDialogBuilder::destructive_confirm("Delete Draft?", "This cannot be undone", "Delete")
        .detail("The draft will be removed from this device.")
)?;

if rx.await? == 1 {
    delete_draft()?;
}
```

`MessageDialogBuilder::confirm(...)` sets Cancel as the escape/cancel action
and OK as the default action. `destructive_confirm(...)` keeps Cancel as the
default/cancel action while returning the destructive button at index `1`.
Custom button layouts can still set `.default_button(index)` and
`.cancel_button(index)` before calling `show_message_dialog(...)`.

Session restore should persist both window state and app-specific workspace
state instead of forcing every app to invent a JSON file alongside window
geometry:

```rust
let store = SessionStore::new("my-app")?;

store.save_snapshot(
    &SessionSnapshotBuilder::new()
        .window_state("main", main_window.window_state())
        .app_data(serde_json::json!({
            "workspace": workspace_id,
            "sidebar": "files",
        }))?
        .build(),
)?;

let displays = cx.displays().iter().map(|display| display.id()).collect::<Vec<_>>();
let primary = cx.primary_display().map(|display| display.id());
let restored_windows = store.restore_window_states(&displays, primary)?;
let snapshot = store.load_snapshot()?;
```

Use `SessionSnapshotBuilder` when restoring Electron-style workspaces, tabs,
sidebar state, recent project ids, or panel layout metadata alongside window
bounds. `save_window_states(...)` and `load_window_states(...)` remain available
for geometry-only apps.

Native menus now have template-style builders over the existing `Menu` and
`MenuItem` tree:

```rust
cx.set_menus_checked(
    MenuBarBuilder::new()
        .menu(
            MenuBuilder::new("File")
                .action("Open...", menu_action::Open)
                .separator()
                .action("Quit", menu_action::Quit),
        )
        .menu(MenuBuilder::new("Edit").action("Copy", menu_action::Copy)),
)?;
```

The checked path rejects empty labels, accidentally padded labels, empty menus,
and duplicate top-level menu names before installing native menus.

Deep links now have a grouped route builder for app startup:

```rust
Application::new()
    .deep_links_checked(
        DeepLinkRouterBuilder::new()
            .route("myapp", |url, cx| {
                println!("app link: {url}");
            })
            .route("oauth", |url, cx| {
                println!("oauth callback: {url}");
            }),
    )?
    .run(|cx| {
        let tasks = cx
            .register_url_schemes(
                UrlSchemeRegistrationBuilder::new()
                    .scheme("myapp")
                    .scheme("oauth"),
            )
            .expect("valid URL schemes");

        for task in tasks {
            task.detach_and_log_err(cx);
        }

        // launch app
    });
```

Use checked grouped routes when handlers are generated from configuration; they
validate scheme syntax and reject duplicate route schemes. Use
`UrlSchemeRegistrationBuilder` when registering multiple custom schemes; it
validates scheme syntax and deduplicates repeated entries before calling the
platform registration API.

Custom app protocols cover the other Electron pattern: serving app-owned
resources such as `app://assets/logo.svg`, internal previews, or generated
documents without leaking raw filesystem paths into UI code.

```rust
let app = Application::new();
app.custom_protocols_checked(
    CustomProtocolRouterBuilder::new()
        .route("app", |request, cx| {
            CustomProtocolResponse::text(format!("path: {}", request.path()))
        }),
)?;
app.run(|cx| {
    if let Some(response) = cx
        .handle_custom_protocol_url("app://assets/readme.txt")
        .expect("valid custom protocol URL")
    {
        println!("served {} bytes", response.body.len());
    }
});
```

The checked router rejects duplicate routes and standard-scheme shadowing
(`http`, `https`, `file`, `data`, `javascript`, etc.). Protocol requests expose
typed `scheme`, `host`, `path`, and `query` fields, and responses validate
status, MIME type, and headers before they are returned.

For the common Electron `protocol.handle("app", ...)` pattern that serves
packaged files, use a checked file resolver instead of manually joining URL
paths:

```rust
let route = CustomProtocolFileResolver::builder("assets/app")
    .host("assets")
    .index_file("index.html")
    .cache_control("public, max-age=60")
    .require_existing_root()
    .canonicalize_root()
    .route_checked("app")?;

app.custom_protocols_checked(CustomProtocolRouterBuilder::from(route))?;
```

The resolver maps `app://assets/...` URLs to files below one root, returns `404`
for missing files or host mismatches, infers common MIME types, and rejects
plain or percent-encoded `..` traversal before reading. Existing files are
canonicalized against the resolver root, so symlink escapes are rejected too.

Single-instance startup now has a named launch result instead of repeating the
same `match` in every app:

```rust
match SingleInstanceBuilder::new("com.example.app").launch()? {
    SingleInstanceLaunch::Primary(instance) => {
        instance.on_activate(Box::new(|| {
            // Focus or reopen the main window.
        }));
    }
    SingleInstanceLaunch::Duplicate { notified, .. } => {
        debug_assert!(notified);
        return Ok(());
    }
}
```

Use the raw `SingleInstance::acquire(...)` and `send_activate_to_existing(...)`
helpers when an app needs custom duplicate-process forwarding.

Shell helpers should use explicit verbs instead of making builders remember
which low-level platform method maps to which OS behavior:

```rust
cx.open_external_url("https://example.com/docs")?;
cx.open_path(project_dir)?;
cx.show_item_in_folder(report_path)?;
cx.open_shell_target(ShellTarget::reveal_path(report_path))?;

cx.open_shell_targets(
    ShellTargetsBuilder::new()
        .url("https://example.com/docs/export")
        .reveal_path(report_path)
        .require_existing_paths(),
)?;

let trash = cx.trash_request_checked(TrashRequest::builder(report_path).canonicalize_path())?;
```

`open_external_url(...)` uses the lower-risk URL capability, while
`open_path(...)`, `show_item_in_folder(...)`, and path/reveal batch targets
require `ShellExecute`. `ShellTargetsBuilder` keeps export/open/reveal workflows
ordered and validated without hand-written loops. It rejects empty or padded
URLs, unsupported shell URL schemes, missing HTTP(S) hosts, empty paths, and NUL
characters. Use `.canonicalize_paths()` when generated open/reveal targets
should be normalized before dispatch. Custom application schemes belong in
`DeepLinkRouterBuilder` / `UrlSchemeRegistrationBuilder`, not accidental shell
execution.
For Electron `shell.trashItem(...)` parity, `TrashRequestBuilder` creates a
checked move-to-trash descriptor that rejects empty paths, NUL bytes, filesystem
roots, relative paths unless opted in, and missing targets by default. It is the
capability-checked handoff for the native trash/recycle backend rather than a
permanent delete operation.

App storage should use checked path roles instead of hard-coded platform
directory guesses:

```rust
let paths = cx.app_paths_checked(
    AppPathBuilder::new("com.example.app")
        .all_common()
        .create_dirs(),
)?;

let settings = paths.config_dir().unwrap().join("settings.json");
let cache_dir = paths.cache_dir().unwrap();
let log_dir = paths.logs_dir().unwrap();
let downloads = paths.downloads_dir().unwrap();
```

This covers the practical `app.getPath(...)` surface Electron apps rely on for
user data, config, cache, logs, temp files, and downloads. `AppPathBuilder`
validates the app id, rejects duplicate roles, scopes app-owned paths by id, and
can create missing directories before migrations, logging, background downloads,
or plugin storage start.

For the storage that Electron apps often leave inside Chromium localStorage,
IndexedDB, or ad hoc profile folders, declare a native app storage plan:

```rust
let storage = cx.app_storage_plan_checked(
    AppStoragePlanBuilder::new("com.example.app")
        .settings_json("settings", "settings.json")
        .sqlite_database("main-db", "state/app.sqlite")
        .blob_cache("previews", "previews")
        .entry(AppStorageEntryBuilder::key_value_store("tokens", "tokens").sensitive()),
)?;
```

`AppStoragePlanBuilder` is not a database engine; it is the checked contract
for where durable settings, SQLite state, key-value data, rebuildable blobs,
logs, and temporary workspaces belong. It resolves the required app path roles,
rejects duplicate ids, unsafe relative paths, parent-directory escapes,
absolute paths, invalid custom kinds, `Downloads` as a storage base, and invalid
quota values. Each entry exposes durability, optional byte budget, sensitivity,
absolute path, and `read_capability()` / `write_capability()` values for worker
or plugin permission wiring. That gives builders and agents a native storage
map instead of assuming a browser profile exists.

Launch context should be explicit and safe for startup routing:

```rust
let launch = cx.launch_context_checked(
    LaunchContextBuilder::new()
        .environment_keys(["APP_CHANNEL", "KAEL_PROFILE"])
        .require_executable()
        .require_current_dir(),
)?;

let args = launch.args();
let channel = launch.env("APP_CHANNEL");
```

This gives generated apps an Electron `process.argv` / process-environment
equivalent without exposing the entire environment by default. Arguments are
captured as UTF-8-lossy strings, environment variables require an explicit
allowlist, duplicate or malformed keys fail early, and apps can require
executable/current-directory resolution when startup routing depends on them.

For Electron `utilityProcess` and `child_process`-style app helpers, describe a
checked helper launch before touching a platform supervisor:

```rust
let launch = HelperProcessLaunch::utility(
    ProcessId(42),
    "video-transcoder",
    cx.path_for_auxiliary_executable("transcoder")?,
)
.arg("--input")
.arg(input_path.display().to_string())
.env("RUST_LOG", "info")
.inherit_environment_keys(["PATH"])
.capabilities(["media:transcode"])
.restart_on_failure(2, Duration::from_millis(250))
.build_checked()?;

let (info, options) = launch.into_spawn_parts();
supervisor.spawn_with_options(info, options)?;
```

`HelperProcessLaunchBuilder` is not a shell-string API. It validates process
class, name, executable, args, explicit env vars, inherited env allowlists,
working directory, declared capability labels, and restart/heartbeat policy.
Use `ProcessClass::Utility` for app-owned native tools that are not UI, media,
extension, or long-running worker hosts. This gives builders an Electron-like
escape hatch for FFmpeg wrappers, language servers, importers, exporters, and
model tools while preserving Kael's native process and permission boundaries.

Locale snapshots cover Electron `app.getLocale()` and preferred-language style
startup choices without using browser APIs:

```rust
let locale = cx.locale_snapshot_checked(
    LocaleSnapshotBuilder::new()
        .preferred_languages(["fr-FR", "en-US"])
)?;

let language = locale.language();
let rtl = locale.is_rtl();
```

The builder normalizes explicit candidates and system signals (`LC_ALL`,
`LC_MESSAGES`, `LANG`, `LANGUAGE`) into BCP-47-style tags, strips encoding and
modifier suffixes, infers region and text direction, and falls back to `en-US`
when the OS exposes only `C`/`POSIX` or no locale data.

Browser text fields also make spelling policy feel automatic in Electron. For
native Kael editors and forms, create a checked text-checking descriptor before
calling an OS or bundled dictionary backend:

```rust
let request = cx.text_checking_request_checked(
    TextCheckingRequestBuilder::new(editor_text)
        .locale_snapshot(&locale)
        .check_grammar()
        .autocorrect()
        .custom_words(["Kael", "GPUI"])
        .max_suggestions(5),
)?;
```

`TextCheckingRequestBuilder` validates text, locale, enabled features, custom
dictionary words, duplicates, and suggestion limits. Gate richer integrations
with `PlatformFeature::SpellChecking`; when it is partial or unavailable, keep
typing usable and omit underline/suggestion UI rather than routing through a
hidden browser field.

Runtime diagnostics should expose current native process cost without requiring
an embedded browser process model:

```rust
let metrics = cx.current_process_metrics();

tracing::info!(
    pid = metrics.process_id(),
    windows = metrics.window_count(),
    rss = ?metrics.resident_set_bytes(),
    uptime_ms = metrics.uptime().as_millis(),
    "desktop resource snapshot"
);
```

This gives builders and agents an Electron `app.getAppMetrics()`-style starting
point for resource audits: process id, uptime, open Kael window count,
executable/current-directory paths, and best-effort memory values. Memory is
reported as optional because each OS exposes low-cost process data differently;
agents should check `metrics.memory().is_supported()` before making hard budget
assertions.

For the "lighter than Electron" promise, use checked resource budgets instead
of informal log inspection:

```rust
let budget = cx.evaluate_resource_budget_checked(
    AppResourceBudgetBuilder::new()
        .max_resident_set_bytes(256 * 1024 * 1024)
        .max_windows(4)
        .require_memory_metrics()
        .warn_when_power_constrained(),
)?;

if !budget.is_within_budget() {
    tracing::warn!(summary = budget.summary(), "resource budget exceeded");
}
```

This gives generated apps and agents a structured runtime gate over current
process metrics plus `runtime_snapshot()`: memory thresholds, window-count
limits, optional uptime limits, required memory-metric availability, and
power/accessibility pressure warnings. It does not replace benchmark evidence,
but it gives each app a cheap guardrail before expensive work, release checks,
or AI-driven changes.

Support diagnostics bundle these native pieces into a privacy-aware support
report for "copy diagnostics", issue templates, and automated bug reports:

```rust
let diagnostics = cx.support_diagnostics_checked(
    SupportDiagnosticsBuilder::new()
        .metadata(
            AppMetadataBuilder::new("Kael Studio")
                .version(env!("CARGO_PKG_VERSION"))
                .identifier("com.example.kael-studio"),
        )
        .app_paths(AppPathBuilder::new("com.example.kael-studio").app_storage()),
)?;

cx.write_clipboard_text(diagnostics.to_text());
```

By default the report includes OS info, locale, current-process metrics,
executable path, current directory, and no argv or environment values. Apps must
opt into `.include_launch_args()` and `.environment_keys([...])`; app paths are
side-effect free and reject `.create_dirs()` so diagnostics cannot mutate the
user's filesystem.

App identity should also be a typed object, not scattered strings in menus,
support pages, and diagnostics:

```rust
let metadata = AppMetadataBuilder::new("Kael Studio")
    .version(env!("CARGO_PKG_VERSION"))
    .build(option_env!("GIT_SHA").unwrap_or("dev"))
    .identifier("com.example.kael-studio")
    .website_url("https://example.com")
    .support_url("https://example.com/support")
    .license("Apache-2.0");

cx.show_about_dialog_checked(metadata)?;
```

This covers the practical Electron app-name/version/About-panel workflow for
generated native apps. `AppMetadataBuilder` validates display names,
version/build labels, identifiers, HTTP(S) support links, copyright, license,
and credits. `AppMetadata::about_dialog()` lets apps route the same validated
metadata through custom menu actions or native message dialogs.

Update UI should have a checked state model even before an app wires a native
installer or custom feed backend:

```rust
let update = cx.app_update_state_checked(
    AppUpdateStateBuilder::new(env!("CARGO_PKG_VERSION"))
        .phase(AppUpdatePhase::Available)
        .release(
            AppUpdateReleaseBuilder::new("1.3.0")
                .channel(AppUpdateChannel::Stable)
                .title("Kael Studio 1.3")
                .notes_url("https://example.com/releases/1.3.0")
                .download_url("https://example.com/downloads/kael-studio-1.3.zip")
                .signed()
                .rollout_percentage(25),
        ),
)?;

let menu_label = update.menu_label();
let action = update.recommended_action();

let decision = cx.app_update_offer_checked(
    AppUpdateOfferPolicyBuilder::stable().cohort_key(machine_install_id),
    AppUpdateReleaseBuilder::new("1.3.0")
        .channel(AppUpdateChannel::Stable)
        .download_url("https://example.com/downloads/kael-studio-1.3.zip")
        .signed()
        .rollout_percentage(25),
)?;
```

This is the honest Electron `autoUpdater` bridge layer today: Kael validates the
state that menus, notifications, settings rows, and agents consume, but it does
not claim to provide a cross-platform installer backend yet. Available,
downloading, downloaded, and ready-to-install phases require release metadata;
download progress is valid only while downloading; failed states require a
sanitized error message; URLs must be HTTP(S).
Use `AppUpdateOfferPolicyBuilder` for the release-eligibility part that would
otherwise become custom updater glue. It checks channel match, rollout
percentage against an explicit bucket or stable cohort key, whether a download
URL is required, and whether a release must be signed before the UI offers it.
Decisions are `Offer`, `Defer`, or `Block`, so agents can distinguish "not for
this channel/cohort yet" from "do not install this release." The `.signed()`
flag is an assertion from the feed/package verifier, not a replacement for
signature verification.

Recent documents now have a builder path over the existing dock/jump-list
integration:

```rust
cx.add_recent_documents(
    RecentDocumentsBuilder::new()
        .require_existing_files()
        .canonicalize()
        .document(report_path)
        .document(notes_path),
).expect("recent document paths");
```

The lower-level `add_recent_document(path)` remains available for one-off
updates, but the builder keeps startup and file-open flows easier for generated
apps to compose. Omit `.require_existing_files()` / `.canonicalize()` when you
want the permissive raw platform behavior.

File watching has checked options for Electron-style project folders, config
files, themes, generated assets, and logs:

```rust
watcher.watch_with_options(
    project_dir,
    FileWatchOptionsBuilder::new()
        .max_depth(3)
        .build_checked()?,
)?;
```

Use `.recursive()` for all descendants, `.max_depth(depth)` for bounded project
watchers, and `.non_recursive()` for single files or direct children. The
checked path rejects zero-depth watches and raw depth limits without recursion
before a platform watcher is registered. Raw `FileWatchOptions { ... }` and
`watch(path, recursive)` remain available for low-level integrations.

App lifecycle policy now has one checked startup path for Electron-style
`window-all-closed`, background app, and bounded cleanup behavior:

```rust
let lifecycle = cx.configure_lifecycle_policy_checked(
    AppLifecyclePolicyBuilder::new()
        .keep_alive_without_windows()
        .quit_cleanup_timeout(Duration::from_millis(500))
        .reason("tray sync stays active"),
)?;
```

Use `.quit_when_all_windows_close()` for normal document or utility apps and
`.keep_alive_without_windows()` for tray, menubar, sync, or agent apps. The
builder applies the platform keep-alive state and the timeout used for
`on_app_quit(...)` cleanup futures together, rejecting zero or longer-than-30s
cleanup windows and invalid diagnostic reasons before lifecycle policy becomes
ambiguous. Raw `set_keep_alive_without_windows(...)`, `on_app_quit(...)`,
`on_app_restart(...)`, and `on_window_closed(...)` remain available for custom
integrations.

App activation and terminal commands also have a checked route for Electron
`app.focus(...)`, `app.hide()`, `app.quit()`, and relaunch-style flows:

```rust
cx.perform_lifecycle_command_checked(
    AppLifecycleCommand::activate_with_options(true)
        .reason("show existing project window"),
)?;

cx.perform_lifecycle_command_checked(
    AppLifecycleCommand::quit("user selected Quit"),
)?;
```

`AppLifecycleCommand::quit(reason)` and `.restart(reason)` require explicit
validated reasons before dispatch, while focus/hide commands may attach optional
diagnostic reasons. Raw `activate(...)`, `hide()`, `hide_other_apps()`,
`unhide_other_apps()`, `quit()`, and `restart()` remain available for already
validated integrations.

For Electron `app.isReady()`-adjacent startup checks and agent audits, read a
single runtime snapshot instead of probing unrelated platform APIs:

```rust
let runtime = cx.runtime_snapshot();

if runtime.is_background_runtime() {
    tracing::info!("tray or agent runtime is active");
}

if runtime.power().should_reduce_work() {
    defer_nonessential_indexing();
}
```

`AppRuntimeSnapshot` includes the capability process id, uptime, window count,
keep-alive policy, quit-cleanup timeout, quitting flag, network status, system
power snapshot, and native theme snapshot. Pair it with
`CapabilityReport::current()`: capability reports say what the desktop can do;
runtime snapshots say what this app process is doing now.

Display queries now have a checked Electron `screen`-style path for palettes,
launchers, inspectors, capture tools, and generated window placement:

```rust
let cursor_display = cx
    .query_displays_checked(DisplayQueryBuilder::cursor().fallback_to_primary())?
    .first()
    .cloned();

let all_displays = cx.query_displays_checked(DisplayQueryBuilder::all())?;
```

`DisplaySnapshot` copies display id, optional stable UUID, bounds, default window
bounds, refresh rate, primary-display state, and cursor containment into a
plain value. Queries can target all displays, the primary display, the
cursor-containing display, or a specific display id, with explicit empty-result
or primary-fallback behavior instead of ad hoc monitor search code.

Window progress has a checked path over the existing taskbar/dock progress
hook:

```rust
window.set_progress_bar_checked(ProgressBarState::normal(0.55)?)?;
window.set_progress_bar_checked(ProgressBarState::Indeterminate)?;
window.set_progress_bar_checked(ProgressBarState::None)?;
```

Use `ProgressBarState::normal(...)`, `error(...)`, and `paused(...)` when the
fraction comes from generated code, transfer progress, export jobs, installers,
or sync state. The checked path rejects NaN, infinity, and values outside
`0.0..=1.0`; the lower-level `window.set_progress_bar(...)` remains available
for already-validated platform-specific state.

Window visibility, focus, minimize, and click-through overlay behavior now have
a checked command path over Electron `BrowserWindow.show()`, `.hide()`,
`.focus()`, `.minimize()`, and `setIgnoreMouseEvents(...)`:

```rust
window.perform_window_interaction_checked(WindowInteractionCommand::show())?;
window.perform_window_interaction_checked(WindowInteractionCommand::activate())?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::mouse_passthrough("HUD overlay should not block clicks"),
)?;
window.perform_window_interaction_checked(WindowInteractionCommand::receive_mouse_events())?;
```

The checked path rejects invalid diagnostic text and requires a reason before
enabling mouse pass-through, which keeps generated overlay windows from
accidentally becoming unclickable. Raw `show_window()`, `hide_window()`,
`activate_window()`, `minimize_window()`, `is_window_visible()`, and
`set_mouse_passthrough(...)` remain available for custom window managers.

For high-performance native UIs that keep many documents, icons, glyphs,
thumbnails, or sprites warm, use a checked atlas budget to keep renderer memory
bounded:

```rust
window.set_atlas_byte_budget_checked(
    WindowAtlasBudgetBuilder::bytes(128 * 1024 * 1024)
        .reason("Large editor view churns text and symbol atlases"),
)?;
window.set_atlas_byte_budget_checked(WindowAtlasBudgetBuilder::clear())?;
```

This gives builders an Electron-alternative memory lever that is native to the
renderer rather than a browser process setting. The checked builder rejects
zero-byte caps, excessively large caps, and invalid diagnostic text; raw
`set_atlas_byte_budget(...)` remains available for platform-owned memory policy.

For frameless/custom-titlebar windows, use checked chrome commands around the
native compositor hooks:

```rust
window.perform_window_chrome_command_checked(
    WindowChromeCommand::request_decorations(WindowDecorations::Client)
        .reason("custom titlebar owns drag regions"),
)?;
window.perform_window_chrome_command_checked(WindowChromeCommand::start_move())?;
window.perform_window_chrome_command_checked(
    WindowChromeCommand::start_resize(ResizeEdge::BottomRight),
)?;
window.perform_window_chrome_command_checked(
    WindowChromeCommand::show_window_menu(point(px(12.0), px(32.0))),
)?;
```

This is the native counterpart to Electron frameless-window drag regions and
system-menu affordances. The checked command rejects invalid diagnostic text and
non-finite menu positions; raw `request_decorations(...)`,
`show_window_menu(...)`, `start_window_move()`, and `start_window_resize(...)`
remain available for already-owned custom chrome.

Dock/taskbar badges now have a checked builder path for counts and short status
labels:

```rust
cx.set_dock_badge_checked(DockBadgeBuilder::count(7))?;
cx.set_dock_badge_checked(DockBadgeBuilder::label("sync"))?;
cx.set_dock_badge_checked(DockBadgeBuilder::clear())?;
```

Use this for unread counts, sync/export status, and generated app chrome where
badge text may come from dynamic state. The checked path rejects empty labels,
padded labels, control characters, and labels longer than 16 characters before
platform badge rendering. Raw `cx.set_dock_badge(Some(label))` and
`cx.set_dock_badge(None)` remain available for already-validated platform state.

Windows jump lists have a builder path for task actions and recent workspace
groups:

```rust
cx.update_jump_list_checked(
    JumpListBuilder::new()
        .action("Open Project", menu_action::Open)
        .workspace_path(project_dir)
        .workspace([project_dir, workspace_file]),
)?;
```

Use `JumpListBuilder` for Electron-style taskbar launchers, recent projects,
and multi-folder workspaces. The checked path rejects empty jump lists,
non-action task menu items, padded/empty action labels, empty workspace entries,
and empty paths; `.require_existing_paths().canonicalize()` is available when a
launcher should only expose real projects. The lower-level
`cx.update_jump_list(menus, entries)` remains available for custom Windows
integrations.

Launch-at-login now follows the same pattern:

```rust
let state = cx.configure_auto_launch(
    AutoLaunchBuilder::enable("com.example.myapp"),
)?;

println!("auto launch enabled: {}", state.enabled());
```

Use `AutoLaunchBuilder::disable(app_id)` for preferences screens. Builder
validation rejects empty app IDs and whitespace/control characters before
platform registration. The raw `set_auto_launch(...)` and
`is_auto_launch_enabled(...)` methods remain available for direct platform
integrations.

Restart paths also have a checked builder for updater, migration, and helper
install flows:

```rust
let config = AutoUpdaterConfigBuilder::new("https://releases.example.com/feed.json")
    .check_interval(Duration::from_secs(86_400))
    .stable_only()
    .build_checked()?;

let updater = AutoUpdater::new_checked(config, current_version, http_client)?;

cx.set_restart_path_checked(
    RestartPathBuilder::current_exe()?
        .require_existing_file()
        .canonicalize(),
)?;
cx.restart();
```

Use `AutoUpdaterConfigBuilder` when generated apps configure update feeds; it
rejects empty or padded feed URLs, invalid URL syntax, non-HTTP(S) schemes,
missing hosts, and zero check intervals before network work begins. Raw
`AutoUpdaterConfig { ... }` and `AutoUpdater::new(...)` remain available for
already-validated updater integrations.

When generated tooling emits update entries, use `UpdateInfoBuilder`:

```rust
let update = UpdateInfoBuilder::new(version, package_url)
    .sha256(package_sha256)
    .size_bytes(package_size)
    .signature(ed25519_signature_base64)
    .build_signed_checked()?;
```

`build_checked()` validates download URL and optional integrity metadata;
`build_signed_checked()` requires signature, SHA-256, and package size before an
entry is treated as signed-update metadata. Raw `UpdateInfo { ... }` remains
available for already-validated feed parsers.

Use `RestartPathBuilder::new(path).require_existing_file().canonicalize()` when
the relaunch target should be a real binary. `.allow_missing()` preserves the raw
platform behavior for custom launchers, and raw `set_restart_path(path)` remains
available for already-validated integrations.

Biometric prompts validate deliberate user-facing reason text, reject accidental
leading/trailing whitespace, and report whether a platform prompt was actually
shown:

```rust
let request = cx.authenticate_biometric_with(
    BiometricAuthBuilder::new("Unlock your vault"),
    |success| {
        if success {
            // Proceed with the sensitive action.
        }
    },
)?;

if !request.prompted() {
    // Fall back to password or PIN.
}
```

The raw `biometric_status()` and `authenticate_biometric(...)` methods remain
available for app-specific flows.

Global hotkeys now have a builder path so apps can parse shortcut strings once
and keep human-readable names beside their numeric IDs:

```rust
cx.register_global_hotkeys_checked(
    GlobalHotkeyBuilder::new()
        .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?
        .parse_named_hotkey(2, "Toggle Capture", "cmd-alt-c")?,
)?;
```

The ID callbacks remain the cross-platform event contract, including Wayland's
portal-backed async binding flow. The checked path rejects empty sets, duplicate
IDs, and duplicate keystrokes before platform registration begins.

Window creation now has a builder path over the raw `WindowOptions` struct, so
agents can express BrowserWindow-style intent without remembering every field:

```rust
cx.open_window(
    WindowIntentBuilder::utility()
        .title("Inspector")
        .windowed(Bounds::centered(None, size(px(900.0), px(640.0)), cx))
        .min_size(size(px(520.0), px(360.0)))
        .build_checked()?,
    |_window, cx| cx.new(|_| InspectorView::new()),
)?;
```

Use `WindowIntentBuilder::{main,palette,utility,modal,popup,overlay}()` first
for generated windows. It composes coherent window kinds, titlebar/background
defaults, resize/minimize/move flags, parent requirements, and placement into
checked raw `WindowOptions`. It rejects invalid bounds/minimum sizes, padded or
control-character titles, path-like app IDs, modal intents without parents,
resizable popups, minimizable palettes, and kind/preset mismatches.
`WindowOptionsBuilder` remains the lower-level escape hatch and preserves the full native option surface: bounds, titlebar,
focus/show behavior, window kind, move/resize/minimize flags, display, native
background appearance, app id, minimum size, decorations, tab groups,
mouse-passthrough overlays, and parent windows.

For Electron fullscreen and kiosk flows, prefer a checked presentation policy
over ad hoc fullscreen toggles:

```rust
window.set_presentation_policy_checked(
    WindowPresentationPolicyBuilder::kiosk("Point of sale checkout"),
)?;
```

Use `WindowPresentationPolicyBuilder::fullscreen(reason)` for presentations,
media playback, onboarding, dashboards, and controlled display surfaces where
the user should keep normal exit behavior. Use
`WindowPresentationPolicyBuilder::kiosk(reason)` for POS and locked-down
workflows that want fullscreen, hidden chrome, and restricted user exit intent.
`clear_presentation_policy_checked()` returns to normal windowed behavior. The
checked path validates reasons, applies platform fullscreen state today, and
records kiosk intent for platform backends that can enforce stronger controls.

After opening a window, prefer
`window.set_app_id_checked(WindowAppIdBuilder::new(app_id))?` for generated
platform grouping IDs and
`window.set_tabbing_identifier_checked(WindowTabbingIdentifierBuilder::new(id))?`
for app-owned macOS tab groups. Use
`WindowTabbingIdentifierBuilder::clear()` to clear tab grouping. The checked
paths reject empty, padded, whitespace-containing, or control-character
identifiers before platform APIs see them; raw `set_app_id(...)` and
`set_tabbing_identifier(...)` remain available for already-validated platform
state.

Document/editor windows should use a checked document state so generated apps do
not forget to keep the user-facing title and unsaved-changes marker together:

```rust
window.set_document_state_checked(
    WindowDocumentStateBuilder::document(project_path.join("Report.md"))
        .require_existing_path()
        .unsaved_changes(),
)?;
```

This is the native-window analogue of Electron document chrome such as
`setDocumentEdited(...)`: it validates explicit titles, derives a title from the
document path, optionally requires/canonicalizes existing paths, and applies the
platform edited marker. Raw `set_window_title(...)` and
`set_window_edited(...)` remain available for already-validated custom flows.

Privacy-sensitive windows should record checked content-protection intent before
platform backends or capture flows decide whether a window can be shared:

```rust
window.set_content_protection_checked(
    WindowContentProtectionBuilder::exclude_from_capture("Protect checkout secrets"),
)?;
```

This is the native-window path for Electron `setContentProtection(true)` use
cases such as auth, checkout, wallets, private documents, unreleased designs,
and confidential diagnostics. Use
`WindowContentProtectionBuilder::obscure_when_captured(...)` when blanking or
blurring captured output is acceptable, and `clear_content_protection_checked()`
when the private flow ends. The checked policy validates a reason and records
whether app-owned window capture should skip the window.

For popovers, tray panels, inspectors, and utility windows, resolve placement
before opening the window:

```rust
let placement = cx.resolve_window_placement(
    WindowPlacementBuilder::new(size(px(420.0), px(320.0)))
        .bottom_right(px(16.0)),
)?;

cx.open_window(
    WindowOptionsBuilder::new()
        .title("Downloads")
        .placement(&placement),
    |_window, cx| cx.new(|_| DownloadsView::new()),
)?;
```

This keeps monitor-aware placement in one validated helper while leaving
`displays()`, `primary_display()`, and `compute_window_bounds(...)` available
for advanced layout code. `WindowOptionsBuilder::placement(&placement)` copies
both the resolved bounds and display id, which is the common tray panel,
popover, palette, and inspector path.

Custom UI accessibility now has semantic recipes for the common controls agents
build by hand:

```rust
let attrs = AccessibilityAttributes::switch("Enable sync", enabled)
    .disabled(is_busy);
attrs.validate()?;
let report = attrs.audit_report();
if !report.is_ready() {
    anyhow::bail!(report.summary());
}

div()
    .track_focus(&focus)
    .tab_stop(true)
    .accessibility(attrs);
```

Recipes cover buttons, links, checkboxes, switches, radio buttons, sliders,
progress bars, and text inputs. The lower-level
`AccessibilityAttributes::new(AccessibilityRole::...)` path remains available
for custom roles and unusual states.
Use `AccessibilityAttributes::audit_report()` for non-throwing component
reviews, and `AccessibilityTree::audit_report()` before platform export when an
app or agent needs to catch all structural issues at once. The tree audit
reports missing children, parent mismatches, multiple focused nodes, hidden
focused nodes, missing interactive names/actions, conflicting states, unknown
roles, and invalid range values.

## Capability documentation gates

Every feature that is sold as "Electron replacement" should have a gate:

| Gate | Evidence required |
| --- | --- |
| API exists | Public docs and examples compile |
| Cross-platform | `video_capability_report()` and platform docs say Full on macOS, Windows, and Linux |
| Graceful fallback | Partial/Unsupported paths are documented |
| Performance | Benchmark against a comparable Electron sample |
| AI-agent ready | `llms.txt` includes the current correct API and an example |
| Production ready | Tests or examples cover failure states, not only happy paths |

Until a gate is green, docs should say "available", "partial", or "roadmap",
not "matches Electron".

For performance evidence, compare a Kael result set against an Electron sample
with `ElectronComparisonReport`:

```rust
let report = ElectronComparisonReport::generate(
    &electron_results,
    kael_harness.results(),
    Some("trace.json".into()),
);

println!("{}", report.summary());
```

Use the same `BenchmarkScenario` and metric names on both sides, and inspect
`BenchmarkScenario::workload_spec()` before publishing a claim. See
[Benchmarking Kael Against Electron](benchmarking.md) for the evidence workflow.

Apps should also gate their own hard requirements before they build a window or
start background work:

```rust
let report = CapabilityReport::current();
let readiness = CapabilityCheck::new()
    .require(PlatformFeature::WebView)
    .require_available(PlatformFeature::Notifications)
    .prefer_available(PlatformFeature::GlobalHotkeys)
    .require(PlatformFeature::PrecisionPointerInput)
    .prefer_available(PlatformFeature::GestureInput)
    .prefer_available(PlatformFeature::TouchInput)
    .prefer_available(PlatformFeature::PenInput)
    .evaluate(&report);

if let Some(summary) = readiness.required_failure_summary() {
    anyhow::bail!("unsupported desktop: {summary}");
}
```

Use `require(...)` for full-support-only requirements, `require_available(...)`
when `Partial` or `RequiresInit` is an acceptable setup/fallback path, and
`prefer_available(...)` for Electron-like conveniences that should produce UI
fallbacks rather than block launch. Input-heavy apps should check
`PrecisionPointerInput`, `GestureInput`, `TouchInput`, and `PenInput` instead
of assuming Chromium-style pointer events are present on every native backend.

## Priority roadmap

P0: truthful positioning and capability matrix.

P1: Electron-easy media: URL in, player out, custom controls optional.

P2: WebView-island recipes for auth, maps, docs, payments, rich editors, and
advanced media.

P3: custom render targets and shaders as the top-level visual escape hatch.
Until that lands, route visuals through the current ladder: styled elements and
`kael_ui`, `canvas(...)` / `PathBuilder`, gradients, SVG, Lottie,
`backdrop_blur(...)` / `effect_layer(...)`, `HeadlessRenderer` for evidence,
and WebView islands for browser-only WebGL/WebGPU content. Use
`graphics_capability_report()` in builder tools and agents so public render
targets/custom shaders stay marked `Roadmap` instead of being sold as shipped
Electron parity.

P4: deeper headless component helpers: focus traps, composite keyboard
interaction, richer a11y action routing, and more prop-builder recipes.
`FocusTrapController` now gives custom modals/popovers/palettes reusable
Tab/Shift-Tab/Escape behavior over Kael's tab-group traversal.
`AccessibilityActionRequest` and `AccessibilityActionRouter` now give
assistive-technology actions a normalized app-routing contract. macOS/Linux
adapter drains preserve those normalized actions against the current tree, and
`Window::on_accessibility_action` / `Window::drain_accessibility_actions` expose
them to app code after each platform tree update. Windows now feeds standard UIA
focus, invoke, toggle, expand/collapse, value, and range-value pattern calls
into that same route; exact edits arrive as `AccessibilityAction::SetValue`
requests with `AccessibilityActionPayload::Value(...)` or
`AccessibilityActionPayload::NumericValue(...)`.
Common custom-control accessibility recipes are now available, but complex
widgets still need more headless guidance.

P5: benchmark suite comparing Kael and Electron sample apps on memory, CPU,
startup, video playback, and idle behavior. The benchmark harness and
`ElectronComparisonReport` now provide the reporting path; the remaining work is
shipping comparable sample apps and publishing measured baselines.

Kael can become a credible Electron replacement by being more honest and more
deliberate than Electron: native by default, web-compatible when needed, and
clear about which rung of the builder ladder solves each problem.
