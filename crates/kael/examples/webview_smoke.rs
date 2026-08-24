// Shared behavioral smoke compiled against both the legacy and GTK4 hosts.
#[cfg(not(any(
    feature = "webview-wayland-gtk4",
    all(
        feature = "webview",
        any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )
    )
)))]
use kael::webview_html;
use kael::{
    App, AppContext, Application, Bounds, Context, InteractiveElement, ParentElement,
    PointerInputEvent, Render, SharedString, Styled, TitlebarOptions, WebViewPageLoadEvent, Window,
    WindowBounds, WindowOptions, div, px, size,
};
#[cfg(any(
    feature = "webview-wayland-gtk4",
    all(
        feature = "webview",
        any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )
    )
))]
use kael::{CustomProtocolResponseBuilder, CustomProtocolRouterBuilder, webview};
#[cfg(all(
    feature = "webview-wayland-gtk4",
    any(target_os = "linux", target_os = "freebsd")
))]
use kael::{TrayMenuItem, point, rgb};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

const WEBVIEW_ID: &str = "webview-smoke";
const SMOKE_HTML: &str = r#"<!doctype html><meta charset="utf-8"><title>Kael WebView smoke</title>
<script src="kael-smoke://assets/data.js"></script><script>
addEventListener('message', event => {
  if (event.data && event.data.kind === 'host-ping') {
    window.gpui.postMessage({ pong: event.data.value });
  }
});
</script>"#;

#[derive(Default)]
struct SmokeProgress {
    javascript: Option<Result<SharedString, SharedString>>,
    url: Option<Result<SharedString, SharedString>>,
    pong_received: bool,
    command_error: Option<SharedString>,
    #[cfg(all(
        feature = "webview-wayland-gtk4",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    raw_platform_handles: bool,
    #[cfg(any(
        feature = "webview-wayland-gtk4",
        all(
            feature = "webview",
            any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd"
            )
        )
    ))]
    custom_protocol: bool,
    #[cfg(all(
        feature = "webview-wayland-gtk4",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    scene_capture: bool,
    #[cfg(all(
        feature = "webview-wayland-gtk4",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    gpu_specs: bool,
    #[cfg(all(
        feature = "webview-wayland-gtk4",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    pointer_lock_protocols: bool,
    #[cfg(all(
        feature = "webview-wayland-gtk4",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    relative_motion_seen: bool,
}

impl SmokeProgress {
    fn record_command(&mut self, result: anyhow::Result<()>) {
        if let Err(error) = result {
            self.command_error
                .get_or_insert_with(|| error.to_string().into());
        }
    }

    fn take_result(&mut self) -> Option<Result<SharedString, SharedString>> {
        if let Some(error) = self.command_error.take() {
            return Some(Err(error));
        }
        if !self.pong_received || self.javascript.is_none() || self.url.is_none() {
            return None;
        }
        #[cfg(all(
            feature = "webview-wayland-gtk4",
            any(target_os = "linux", target_os = "freebsd")
        ))]
        if !self.raw_platform_handles
            || !self.scene_capture
            || !self.gpu_specs
            || !self.pointer_lock_protocols
        {
            return None;
        }
        #[cfg(any(
            feature = "webview-wayland-gtk4",
            all(
                feature = "webview",
                any(
                    target_os = "macos",
                    target_os = "windows",
                    target_os = "linux",
                    target_os = "freebsd"
                )
            )
        ))]
        if !self.custom_protocol {
            return None;
        }

        let javascript = match self.javascript.take().expect("checked above") {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        let url = match self.url.take().expect("checked above") {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        if url.is_empty() {
            return Some(Err("WebView returned an empty current URL".into()));
        }
        Some(Ok(format!("{javascript}|url={url}|pong=42").into()))
    }
}

struct WebViewSmoke {
    progress: Arc<Mutex<SmokeProgress>>,
}

#[cfg(all(
    feature = "webview-wayland-gtk4",
    any(target_os = "linux", target_os = "freebsd")
))]
struct SceneCaptureSmoke;

#[cfg(all(
    feature = "webview-wayland-gtk4",
    any(target_os = "linux", target_os = "freebsd")
))]
impl Render for SceneCaptureSmoke {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl kael::IntoElement {
        div()
            .size_full()
            .bg(rgb(0x10192d))
            .text_color(rgb(0xc9e7ff))
            .p_8()
            .child("Kael GTK4 retained-scene PNG capture")
    }
}

impl Render for WebViewSmoke {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl kael::IntoElement {
        let progress = self.progress.clone();
        let progress_after_load = self.progress.clone();
        let _pointer_progress = self.progress.clone();
        #[cfg(any(
            feature = "webview-wayland-gtk4",
            all(
                feature = "webview",
                any(
                    target_os = "macos",
                    target_os = "windows",
                    target_os = "linux",
                    target_os = "freebsd"
                )
            )
        ))]
        let smoke_webview = webview(WEBVIEW_ID, "kael-smoke://assets/probe");
        #[cfg(not(any(
            feature = "webview-wayland-gtk4",
            all(
                feature = "webview",
                any(
                    target_os = "macos",
                    target_os = "windows",
                    target_os = "linux",
                    target_os = "freebsd"
                )
            )
        )))]
        let smoke_webview = webview_html(WEBVIEW_ID, SMOKE_HTML);
        div()
            .size_full()
            .on_pointer_event(move |event: &PointerInputEvent, _, _| {
                if f32::from(event.movement.x) != 0.0 || f32::from(event.movement.y) != 0.0 {
                    #[cfg(all(
                        feature = "webview-wayland-gtk4",
                        any(target_os = "linux", target_os = "freebsd")
                    ))]
                    {
                        _pointer_progress
                            .lock()
                            .expect("smoke progress mutex")
                            .relative_motion_seen = true;
                    }
                }
            })
            .child(
                smoke_webview
                    .on_message(move |message, window, _| {
                        #[cfg(any(
                            feature = "webview-wayland-gtk4",
                            all(
                                feature = "webview",
                                any(
                                    target_os = "macos",
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "freebsd"
                                )
                            )
                        ))]
                        if message.get("protocol").and_then(|value| value.as_str())
                            == Some("custom-protocol-ok")
                            && message
                                .get("protocolStatus")
                                .and_then(|value| value.as_u64())
                                == Some(200)
                            && message
                                .get("protocolHeader")
                                .and_then(|value| value.as_str())
                                == Some("served")
                        {
                            println!("WEBVIEW_SMOKE_STAGE: custom-protocol");
                            progress
                                .lock()
                                .expect("smoke progress mutex")
                                .custom_protocol = true;
                            return;
                        }
                        #[cfg(any(
                            feature = "webview-wayland-gtk4",
                            all(
                                feature = "webview",
                                any(
                                    target_os = "macos",
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "freebsd"
                                )
                            )
                        ))]
                        if let Some(error) = message
                            .get("protocolError")
                            .and_then(|value| value.as_str())
                        {
                            progress.lock().expect("smoke progress mutex").command_error =
                                Some(format!("custom protocol fetch failed: {error}").into());
                            return;
                        }
                        if message.get("pong").and_then(|value| value.as_u64()) == Some(42) {
                            println!("WEBVIEW_SMOKE_STAGE: host-message-round-trip");
                            progress.lock().expect("smoke progress mutex").pong_received = true;
                            return;
                        }
                        if message.get("ready").and_then(|ready| ready.as_bool()) == Some(true) {
                            println!("WEBVIEW_SMOKE_STAGE: page-to-host-ipc");
                            let javascript_progress = progress.clone();
                            let javascript_result = window.evaluate_webview_javascript_with_result(
                                WEBVIEW_ID,
                                "({ value: document.title + ':' + String(6 * 7) })",
                                move |result| {
                                    println!("WEBVIEW_SMOKE_STAGE: javascript-result");
                                    javascript_progress
                                        .lock()
                                        .expect("smoke progress mutex")
                                        .javascript = Some(result)
                                },
                            );
                            progress
                                .lock()
                                .expect("smoke progress mutex")
                                .record_command(javascript_result);

                            let url_progress = progress.clone();
                            let read_url_result =
                                window.read_webview_url(WEBVIEW_ID, move |result| {
                                    println!("WEBVIEW_SMOKE_STAGE: current-url");
                                    url_progress.lock().expect("smoke progress mutex").url =
                                        Some(result)
                                });
                            progress
                                .lock()
                                .expect("smoke progress mutex")
                                .record_command(read_url_result);

                            for command in [
                                window.set_webview_zoom_factor(WEBVIEW_ID, 1.125),
                                window.focus_webview(WEBVIEW_ID),
                                window.focus_webview_parent(WEBVIEW_ID),
                                window.post_webview_message(
                                    WEBVIEW_ID,
                                    serde_json::json!({ "kind": "host-ping", "value": 42 }),
                                ),
                            ] {
                                progress
                                    .lock()
                                    .expect("smoke progress mutex")
                                    .record_command(command);
                            }
                        }
                    })
                    .on_page_load(move |event, _, window, _| {
                        if event == WebViewPageLoadEvent::Finished {
                            println!("WEBVIEW_SMOKE_STAGE: page-load-finished");
                            if let Err(error) = window.evaluate_webview_javascript(
                                WEBVIEW_ID,
                                "window.gpui.postMessage({ ready: true, value: 42 })",
                            ) {
                                progress_after_load
                                    .lock()
                                    .expect("smoke progress mutex")
                                    .command_error = Some(error.to_string().into());
                            }
                        }
                    })
                    .native_permission_policy(|_| kael::WebViewPermissionDecision::Deny)
                    .opacity(0.96)
                    .size_full(),
            )
    }
}

fn main() -> anyhow::Result<()> {
    let _ = env_logger::try_init();
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    println!("WEBVIEW_SMOKE_BACKEND: {}", kael::guess_compositor());

    let progress = Arc::new(Mutex::new(SmokeProgress::default()));
    let outcome = Arc::new(AtomicU8::new(0));
    let outcome_for_app = outcome.clone();
    let application = Application::try_new()?;
    #[cfg(any(
        feature = "webview-wayland-gtk4",
        all(
            feature = "webview",
            any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd"
            )
        )
    ))]
    application.custom_protocols_checked(CustomProtocolRouterBuilder::new().route(
        "kael-smoke",
        |request, _| {
            let (mime_type, body) = if request.path() == "/probe" {
                ("text/html; charset=utf-8", SMOKE_HTML.as_bytes().to_vec())
            } else if request.path() == "/data.js" {
                (
                    "text/javascript; charset=utf-8",
                    b"window.gpui.postMessage({ protocol: 'custom-protocol-ok', protocolStatus: 200, protocolHeader: 'served' });".to_vec(),
                )
            } else {
                ("text/plain; charset=utf-8", b"unexpected-path".to_vec())
            };
            CustomProtocolResponseBuilder::new(mime_type)
                .header("X-Kael-Probe", "served")
                .header("Cache-Control", "no-store")
                .body(body)
                .build_checked()
        },
    ))?;
    application.run(move |cx: &mut App| {
        let webview_progress = progress.clone();
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(480.0), px(320.0)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kael GTK4 WebView smoke".into()),
                    ..Default::default()
                }),
                // Request visibility after `open_window` returns so every backend
                // exercises its public show lifecycle before the runtime checks.
                show: false,
                ..Default::default()
            },
            move |_, cx| {
                let progress = webview_progress;
                cx.new(move |_| WebViewSmoke { progress })
            },
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("WEBVIEW_SMOKE_FAIL: could not open window: {error:#}");
                outcome_for_app.store(2, Ordering::Release);
                cx.quit();
                return;
            }
        };

        if let Err(error) = window.update(cx, |_, window, _| {
            window.show_window();
            window.refresh();
        }) {
            eprintln!("WEBVIEW_SMOKE_FAIL: could not schedule initial frame: {error:#}");
            outcome_for_app.store(2, Ordering::Release);
            cx.quit();
            return;
        }

        #[cfg(all(
            feature = "webview-wayland-gtk4",
            any(target_os = "linux", target_os = "freebsd")
        ))]
        {
            cx.show_context_menu(
                point(px(12.0), px(12.0)),
                vec![
                    TrayMenuItem::action("Open résumé", "open"),
                    TrayMenuItem::separator(),
                    TrayMenuItem::toggle("Snap to grid", true, "snap"),
                    TrayMenuItem::submenu(
                        "Insert",
                        vec![TrayMenuItem::action("Σ formula", "formula")],
                    ),
                ],
                |_, _| {},
            );
            println!("WEBVIEW_SMOKE_STAGE: native-context-menu");
        }

        #[cfg(all(
            feature = "webview-wayland-gtk4",
            any(target_os = "linux", target_os = "freebsd")
        ))]
        let capture_window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(360.0), px(220.0)),
                    cx,
                ))),
                titlebar: None,
                show: false,
                ..Default::default()
            },
            |_, cx| cx.new(|_| SceneCaptureSmoke),
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("WEBVIEW_SMOKE_FAIL: could not open capture window: {error:#}");
                outcome_for_app.store(2, Ordering::Release);
                cx.quit();
                return;
            }
        };
        #[cfg(all(
            feature = "webview-wayland-gtk4",
            any(target_os = "linux", target_os = "freebsd")
        ))]
        if let Err(error) = capture_window.update(cx, |_, window, _| {
            window.show_window();
            window.refresh();
        }) {
            eprintln!("WEBVIEW_SMOKE_FAIL: could not show capture window: {error:#}");
            outcome_for_app.store(2, Ordering::Release);
            cx.quit();
            return;
        }

        let outcome = outcome_for_app;
        #[cfg(all(
            feature = "webview-wayland-gtk4",
            any(target_os = "linux", target_os = "freebsd")
        ))]
        let progress_for_capture = progress;
        cx.spawn(async move |cx| {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            #[cfg(all(
                feature = "webview-wayland-gtk4",
                any(target_os = "linux", target_os = "freebsd")
            ))]
            let mut last_capture_error = None;
            loop {
                #[cfg(all(
                    feature = "webview-wayland-gtk4",
                    any(target_os = "linux", target_os = "freebsd")
                ))]
                if !progress_for_capture
                    .lock()
                    .expect("smoke progress mutex")
                    .scene_capture
                {
                    match capture_window.update(cx, |_, window, _| window.export_frame_png()) {
                        Ok(Ok(image))
                            if image.format() == kael::ImageFormat::Png
                                && image.bytes().starts_with(b"\x89PNG\r\n\x1a\n") =>
                        {
                            println!("WEBVIEW_SMOKE_STAGE: retained-scene-png");
                            progress_for_capture
                                .lock()
                                .expect("smoke progress mutex")
                                .scene_capture = true;
                        }
                        Ok(Ok(_)) => {
                            last_capture_error = Some("capture returned a non-PNG image".to_string());
                        }
                        Ok(Err(error)) => last_capture_error = Some(error.to_string()),
                        Err(error) => last_capture_error = Some(error.to_string()),
                    }
                }
                #[cfg(all(
                    feature = "webview-wayland-gtk4",
                    any(target_os = "linux", target_os = "freebsd")
                ))]
                if !progress_for_capture
                    .lock()
                    .expect("smoke progress mutex")
                    .gpu_specs
                {
                    if let Ok(Some(specs)) = capture_window.update(cx, |_, window, _| {
                        window.gpu_specs().filter(|specs| {
                            !specs.device_name.is_empty()
                                && !specs.driver_name.is_empty()
                                && !specs.driver_info.is_empty()
                        })
                    }) {
                        println!(
                            "WEBVIEW_SMOKE_STAGE: gsk-gpu-specs {}",
                            specs.device_name
                        );
                        progress_for_capture
                            .lock()
                            .expect("smoke progress mutex")
                            .gpu_specs = true;
                    }
                }
                if let Ok(Some(result)) = window.update(cx, |view, _platform_window, _| {
                    let mut progress = view.progress.lock().expect("smoke progress mutex");
                    #[cfg(all(
                        feature = "webview-wayland-gtk4",
                        any(target_os = "linux", target_os = "freebsd")
                    ))]
                    if !progress.raw_platform_handles {
                        let window_handle =
                            raw_window_handle::HasWindowHandle::window_handle(_platform_window);
                        let display_handle =
                            raw_window_handle::HasDisplayHandle::display_handle(_platform_window);
                        let compositor = kael::guess_compositor();
                        progress.raw_platform_handles = match compositor {
                            "Wayland" => {
                                window_handle.is_ok_and(|handle| {
                                    matches!(
                                        handle.as_raw(),
                                        raw_window_handle::RawWindowHandle::Wayland(_)
                                    )
                                }) && display_handle.is_ok_and(|handle| {
                                    matches!(
                                        handle.as_raw(),
                                        raw_window_handle::RawDisplayHandle::Wayland(_)
                                    )
                                })
                            }
                            "X11" => {
                                window_handle.is_ok_and(|handle| {
                                    matches!(
                                        handle.as_raw(),
                                        raw_window_handle::RawWindowHandle::Xlib(_)
                                    )
                                }) && display_handle.is_ok_and(|handle| {
                                    matches!(
                                        handle.as_raw(),
                                        raw_window_handle::RawDisplayHandle::Xlib(_)
                                    )
                                })
                            }
                            _ => false,
                        };
                        if progress.raw_platform_handles {
                            println!("WEBVIEW_SMOKE_STAGE: raw-platform-handles-{compositor}");
                        }
                    }
                    #[cfg(all(
                        feature = "webview-wayland-gtk4",
                        any(target_os = "linux", target_os = "freebsd")
                    ))]
                    if !progress.pointer_lock_protocols
                        && _platform_window.game_input_capabilities().pointer_lock
                            == kael::GameInputAvailability::Available
                    {
                        println!("WEBVIEW_SMOKE_STAGE: native-pointer-lock");
                        progress.pointer_lock_protocols = true;
                    }
                    progress.take_result()
                }) {
                    match result {
                        Ok(value) if value.as_ref().contains("Kael WebView smoke:42") => {
                            #[cfg(all(
                                feature = "webview-wayland-gtk4",
                                any(target_os = "linux", target_os = "freebsd")
                            ))]
                            {
                                let require_pointer_motion = std::env::var_os(
                                    "KAEL_WEBVIEW_SMOKE_REQUIRE_POINTER_MOTION",
                                )
                                .is_some();
                                if require_pointer_motion
                                    || std::env::var_os(
                                        "KAEL_WEBVIEW_SMOKE_REQUIRE_POINTER_LOCK",
                                    )
                                    .is_some()
                                {
                                println!("WEBVIEW_SMOKE_STAGE: pointer-lock-focus-ready");
                                cx.background_executor()
                                    .timer(Duration::from_millis(350))
                                    .await;
                                let request = window.update(cx, |_, window, _| {
                                    window
                                        .request_pointer_lock()
                                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                                    anyhow::ensure!(
                                        window.pointer_lock_status()
                                            == kael::PointerLockStatus::Locked,
                                        "X11 pointer lock was not acquired synchronously"
                                    );
                                    Ok::<(), anyhow::Error>(())
                                });
                                let request = match request {
                                    Ok(result) => result,
                                    Err(error) => Err(anyhow::anyhow!(
                                        "updating the pointer-lock smoke window: {error}"
                                    )),
                                };
                                if let Err(error) = request {
                                    eprintln!(
                                        "WEBVIEW_SMOKE_FAIL: real pointer-lock request failed: {error:#}"
                                    );
                                    outcome.store(2, Ordering::Release);
                                } else {
                                    println!("WEBVIEW_SMOKE_STAGE: pointer-lock-acquired");
                                    if require_pointer_motion {
                                        println!("WEBVIEW_SMOKE_STAGE: locked-awaiting-motion");
                                        let motion_deadline = std::time::Instant::now()
                                            + Duration::from_secs(5);
                                        while !progress_for_capture
                                            .lock()
                                            .expect("smoke progress mutex")
                                            .relative_motion_seen
                                        {
                                            if std::time::Instant::now() >= motion_deadline {
                                                eprintln!(
                                                    "WEBVIEW_SMOKE_FAIL: XI2 relative motion was not delivered"
                                                );
                                                outcome.store(2, Ordering::Release);
                                                break;
                                            }
                                            cx.background_executor()
                                                .timer(Duration::from_millis(25))
                                                .await;
                                        }
                                    }
                                    let release = window.update(cx, |_, window, _| {
                                        window.exit_pointer_lock().map_err(|error| {
                                            anyhow::anyhow!(error.to_string())
                                        })?;
                                        anyhow::ensure!(
                                            window.pointer_lock_status()
                                                == kael::PointerLockStatus::Unlocked,
                                            "X11 pointer lock did not release"
                                        );
                                        Ok::<(), anyhow::Error>(())
                                    });
                                    let release = match release {
                                        Ok(result) => result,
                                        Err(error) => Err(anyhow::anyhow!(
                                            "updating the pointer-lock release: {error}"
                                        )),
                                    };
                                    if let Err(error) = release {
                                        eprintln!(
                                            "WEBVIEW_SMOKE_FAIL: pointer-lock release failed: {error:#}"
                                        );
                                        outcome.store(2, Ordering::Release);
                                    } else {
                                        println!(
                                            "WEBVIEW_SMOKE_STAGE: pointer-lock-acquire-and-release"
                                        );
                                        if require_pointer_motion
                                            && outcome.load(Ordering::Acquire) != 2
                                        {
                                            println!(
                                                "WEBVIEW_SMOKE_STAGE: relative-motion-and-release"
                                            );
                                        }
                                    }
                                }
                            }
                            }
                            if outcome.load(Ordering::Acquire) != 2 {
                                println!("WEBVIEW_SMOKE_OK: {value}");
                                outcome.store(1, Ordering::Release);
                            }
                        }
                        Ok(value) => {
                            eprintln!("WEBVIEW_SMOKE_FAIL: unexpected JavaScript result: {value}");
                            outcome.store(2, Ordering::Release);
                        }
                        Err(error) => {
                            eprintln!("WEBVIEW_SMOKE_FAIL: JavaScript evaluation failed: {error}");
                            outcome.store(2, Ordering::Release);
                        }
                    }
                    #[cfg(all(
                        feature = "webview-wayland-gtk4",
                        any(target_os = "linux", target_os = "freebsd")
                    ))]
                    if outcome.load(Ordering::Acquire) == 1 {
                        // Native input intentionally keeps frame delivery warm
                        // for one second to minimize follow-up latency. Let
                        // that bounded window expire before measuring steady
                        // idle behavior; otherwise a fast incremental build
                        // is paradoxically more likely to fail than a cold CI
                        // build.
                        cx.background_executor()
                            .timer(Duration::from_millis(1_100))
                            .await;
                        let idle_wakeups_before = kael::gtk4_wayland_event_wakeup_count();
                        let idle_frame_ticks_before = kael::gtk4_wayland_frame_tick_count();
                        // One event-driven wakeup resumes this timer. The old
                        // 2ms poller would add roughly 125 wakeups while idle.
                        cx.background_executor()
                            .timer(Duration::from_millis(250))
                            .await;
                        let idle_wakeups_after = kael::gtk4_wayland_event_wakeup_count();
                        let idle_delta = idle_wakeups_after.saturating_sub(idle_wakeups_before);
                        let idle_frame_ticks_after = kael::gtk4_wayland_frame_tick_count();
                        let idle_frame_delta =
                            idle_frame_ticks_after.saturating_sub(idle_frame_ticks_before);
                        if idle_delta <= 2 && idle_frame_delta <= 2 {
                            println!("WEBVIEW_SMOKE_STAGE: idle-event-driven");
                        } else {
                            eprintln!(
                                "WEBVIEW_SMOKE_FAIL: GTK4 bridge woke {idle_delta} times and ran {idle_frame_delta} frame ticks during a 250ms idle interval"
                            );
                            outcome.store(2, Ordering::Release);
                        }
                    }
                    let _ = cx.update(|cx| cx.quit());
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    eprintln!(
                        "WEBVIEW_SMOKE_FAIL: timed out waiting for IPC/JavaScript/capture round trip"
                    );
                    #[cfg(all(
                        feature = "webview-wayland-gtk4",
                        any(target_os = "linux", target_os = "freebsd")
                    ))]
                    if let Some(error) = last_capture_error {
                        eprintln!("WEBVIEW_SMOKE_CAPTURE_FAIL: {error}");
                    }
                    outcome.store(2, Ordering::Release);
                    let _ = cx.update(|cx| cx.quit());
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
            }
        })
        .detach();
    });
    anyhow::ensure!(
        outcome.load(Ordering::Acquire) == 1,
        "WebView smoke test failed"
    );
    Ok(())
}
