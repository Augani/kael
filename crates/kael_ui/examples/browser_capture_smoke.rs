#[cfg(not(target_arch = "wasm32"))]
fn main() {}

#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(run());
}

#[cfg(target_arch = "wasm32")]
async fn run() {
    use kael::{
        CaptureConfig, CaptureDeviceKind, CaptureFrame, CaptureSessionState, FrameCallback,
        PixelFormat, default_capture_manager,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };
    use std::time::Duration;

    let manager = default_capture_manager();
    let enumerated = [
        CaptureDeviceKind::Screen,
        CaptureDeviceKind::Window,
        CaptureDeviceKind::Camera,
    ]
    .into_iter()
    .all(|kind| {
        manager
            .devices(kind)
            .is_ok_and(|devices| devices.len() == 1 && devices[0].kind == kind)
    });

    let config = CaptureConfig::new("browser-display-picker", CaptureDeviceKind::Screen)
        .frame_rate(30.0)
        .resolution(64, 32);
    let mut session = match manager.create_session(&config) {
        Ok(session) => session,
        Err(_) => {
            report(
                false, false, false, false, false, false, false, false, false, false, false, false,
                0, 0,
            );
            return;
        }
    };
    let frames = Arc::new(AtomicU64::new(0));
    let valid_frame = Arc::new(AtomicBool::new(false));
    let validity_mask = Arc::new(AtomicU64::new(0));
    let callback_frames = Arc::clone(&frames);
    let callback_valid = Arc::clone(&valid_frame);
    let callback_validity_mask = Arc::clone(&validity_mask);
    let callback: FrameCallback = Arc::new(move |frame| {
        if let CaptureFrame::Video {
            width,
            height,
            format,
            data,
            timestamp_ms: _,
        } = frame
        {
            let expected_len = usize::try_from(width)
                .ok()
                .and_then(|width| {
                    usize::try_from(height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(4));
            let mut mask = 0;
            mask |= u64::from(width == 64);
            mask |= u64::from(height == 32) << 1;
            mask |= u64::from(format == PixelFormat::Rgba32) << 2;
            mask |= u64::from(expected_len == Some(data.len())) << 3;
            mask |= u64::from(data.iter().skip(3).step_by(4).all(|alpha| *alpha == 255)) << 4;
            callback_validity_mask.store(mask, Ordering::Release);
            callback_valid.store(mask == 0b1_1111, Ordering::Release);
            callback_frames.fetch_add(1, Ordering::AcqRel);
        }
    });

    let started =
        session.start(config, callback).is_ok() && session.state() == CaptureSessionState::Starting;
    wait_until(Duration::from_secs(5), || {
        frames.load(Ordering::Acquire) > 0
    })
    .await;
    let delivered = valid_frame.load(Ordering::Acquire)
        && frames.load(Ordering::Acquire) > 0
        && session.state() == CaptureSessionState::Running;
    let synthetic_fixture_limited = !delivered && live_undecodable_canvas_fixture();

    let before_pause = frames.load(Ordering::Acquire);
    let paused = session.pause().is_ok() && session.state() == CaptureSessionState::Paused;
    wait_for(Duration::from_millis(150)).await;
    let pause_held = frames.load(Ordering::Acquire) == before_pause;
    let resumed = session.resume().is_ok();
    wait_until(Duration::from_secs(2), || {
        frames.load(Ordering::Acquire) > before_pause
    })
    .await;
    let resumed_delivery = frames.load(Ordering::Acquire) > before_pause;
    let resumed_running = session.state() == CaptureSessionState::Running;
    let stopped = session.stop().is_ok() && session.state() == CaptureSessionState::Stopped;
    let lifecycle = paused
        && pause_held
        && resumed
        && resumed_running
        && (resumed_delivery || synthetic_fixture_limited)
        && stopped;

    let noop: FrameCallback = Arc::new(|_| {});
    let invalid_audio =
        CaptureConfig::new("browser-display-picker", CaptureDeviceKind::Screen).include_audio(true);
    let audio_rejected = manager
        .create_session(&invalid_audio)
        .and_then(|mut session| session.start(invalid_audio, Arc::clone(&noop)))
        .is_err();
    let invalid_bounds = CaptureConfig::new("browser-display-picker", CaptureDeviceKind::Screen)
        .resolution(100_000, 100_000);
    let bounds_rejected = manager
        .create_session(&invalid_bounds)
        .and_then(|mut session| session.start(invalid_bounds, Arc::clone(&noop)))
        .is_err();
    let bounds = audio_rejected && bounds_rejected;

    set_reject_next_capture();
    let rejected_config = CaptureConfig::new("browser-display-picker", CaptureDeviceKind::Screen);
    let async_error = match manager.create_session(&rejected_config) {
        Ok(mut rejected_session) => {
            let invoked = rejected_session.start(rejected_config, noop).is_ok();
            wait_until(Duration::from_secs(2), || {
                rejected_session.state() == CaptureSessionState::Error
            })
            .await;
            invoked
                && rejected_session.state() == CaptureSessionState::Error
                && rejected_session.last_error().is_some()
        }
        Err(_) => false,
    };

    report(
        enumerated,
        started,
        delivered,
        lifecycle,
        bounds,
        async_error,
        synthetic_fixture_limited,
        paused,
        pause_held,
        resumed,
        resumed_running,
        stopped,
        frames.load(Ordering::Acquire),
        validity_mask.load(Ordering::Acquire),
    );
}

#[cfg(target_arch = "wasm32")]
fn live_undecodable_canvas_fixture() -> bool {
    use wasm_bindgen::{JsCast as _, JsValue};
    use web_sys::HtmlVideoElement;

    let Some(window) = web_sys::window() else {
        return false;
    };
    if js_sys::Reflect::get(
        window.as_ref(),
        &JsValue::from_str("__kaelCanvasCaptureFixture"),
    )
    .ok()
    .and_then(|value| value.as_bool())
        != Some(true)
    {
        return false;
    }
    let Ok(Some(element)) = window
        .document()
        .and_then(|document| document.query_selector("video").ok())
        .ok_or(())
    else {
        return false;
    };
    let Ok(video) = element.dyn_into::<HtmlVideoElement>() else {
        return false;
    };
    let Some(stream) = video.src_object() else {
        return false;
    };
    let tracks = stream.get_video_tracks();
    video.video_width() == 0
        && video.video_height() == 0
        && !video.paused()
        && video.current_time() >= 0.5
        && tracks.length() == 1
        && js_sys::Reflect::get(&tracks.get(0), &JsValue::from_str("readyState"))
            .ok()
            .and_then(|value| value.as_string())
            .as_deref()
            == Some("live")
}

#[cfg(target_arch = "wasm32")]
async fn wait_until(timeout: std::time::Duration, mut predicate: impl FnMut() -> bool) {
    let started = web_time::Instant::now();
    while !predicate() && started.elapsed() < timeout {
        wait_for(std::time::Duration::from_millis(20)).await;
    }
}

#[cfg(target_arch = "wasm32")]
async fn wait_for(duration: std::time::Duration) {
    use futures::channel::oneshot;
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::{JsCast as _, closure::Closure};

    let (sender, receiver) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let callback_sender = Rc::clone(&sender);
    let callback = Closure::<dyn FnMut()>::new(move || {
        if let Some(sender) = callback_sender.borrow_mut().take() {
            let _ = sender.send(());
        }
    });
    let Some(window) = web_sys::window() else {
        return;
    };
    let timeout = i32::try_from(duration.as_millis()).unwrap_or(i32::MAX);
    let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        timeout,
    ) else {
        return;
    };
    let _ = receiver.await;
    window.clear_timeout_with_handle(id);
    drop(callback);
}

#[cfg(target_arch = "wasm32")]
fn set_reject_next_capture() {
    use wasm_bindgen::JsValue;

    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(
            window.as_ref(),
            &JsValue::from_str("__kaelRejectNextCapture"),
            &JsValue::TRUE,
        );
    }
}

#[cfg(target_arch = "wasm32")]
fn report(
    enumerated: bool,
    started: bool,
    delivered: bool,
    lifecycle: bool,
    bounds: bool,
    async_error: bool,
    synthetic_fixture_limited: bool,
    paused: bool,
    pause_held: bool,
    resumed: bool,
    resumed_running: bool,
    stopped: bool,
    frame_count: u64,
    validity_mask: u64,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let frame_contract = delivered || synthetic_fixture_limited;
    let passed = enumerated && started && frame_contract && lifecycle && bounds && async_error;
    let marker = if passed {
        "?__kael_capture_pass__=1"
    } else {
        "?__kael_capture_failed__=1"
    };
    let query = format!(
        "{marker}&enumeration={}&start={}&frames={}&lifecycle={}&bounds={}&async_error={}&fixture={}&pause={}&pause_hold={}&resume={}&resume_state={}&stop={}&frame_count={frame_count}&validity_mask={validity_mask}",
        status(enumerated),
        status(started),
        if delivered {
            "passed"
        } else if synthetic_fixture_limited {
            "automation-unavailable"
        } else {
            "failed"
        },
        status(lifecycle),
        status(bounds),
        status(async_error),
        if synthetic_fixture_limited {
            "live-track-no-decodable-frames"
        } else {
            "frame-delivery"
        },
        status(paused),
        status(pause_held),
        status(resumed),
        status(resumed_running),
        status(stopped),
    );
    let _ = window.location().set_search(&query);
}

#[cfg(target_arch = "wasm32")]
fn status(passed: bool) -> &'static str {
    if passed { "passed" } else { "failed" }
}
