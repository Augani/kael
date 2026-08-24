//! Browser video and display capture backend.

use super::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureFrame,
    CaptureSession, CaptureSessionState, DeviceEnumerator, FrameCallback, PixelFormat,
};
use anyhow::{Context as _, Result, anyhow};
use js_sys::{Function, Promise};
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    CanvasRenderingContext2d, Event, HtmlCanvasElement, HtmlVideoElement, MediaStream,
    MediaStreamConstraints, MediaStreamTrack,
};

const MAX_BROWSER_CAPTURE_PIXELS: u64 = 16_777_216;
const MAX_BROWSER_CAPTURE_FPS: f64 = 240.0;
// Bound readback/copy pressure while still allowing 4K at 60 Hz or 1080p at
// high refresh rates. Larger captures are sampled less frequently rather than
// allocating multiple gigabytes per second on the browser main thread.
const MAX_BROWSER_CAPTURE_PIXELS_PER_SECOND: f64 = 536_870_912.0;
static NEXT_BROWSER_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static BROWSER_CAPTURE_RUNTIMES: RefCell<HashMap<u64, BrowserCaptureRuntime>> =
        RefCell::new(HashMap::new());
}

struct BrowserCaptureRuntime {
    _stream: MediaStream,
    tracks: Vec<MediaStreamTrack>,
    video: HtmlVideoElement,
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    frame_callback: Closure<dyn FnMut(f64)>,
    _ended_callbacks: Vec<Closure<dyn FnMut(Event)>>,
    callback: FrameCallback,
    state: Arc<AtomicU8>,
    error: Arc<Mutex<Option<String>>>,
    dropped: Arc<AtomicU64>,
    total_latency_ms: Arc<AtomicU64>,
    frame_count: Arc<AtomicU64>,
    frame_request: Option<i32>,
    target_interval_ms: f64,
    last_frame_ms: Option<f64>,
    requested_resolution: Option<(u32, u32)>,
}

impl BrowserCaptureRuntime {
    fn shutdown(mut self) {
        if let Some(request) = self.frame_request.take()
            && let Some(window) = web_sys::window()
        {
            let _ = window.cancel_animation_frame(request);
        }
        for track in &self.tracks {
            track.set_onended(None);
            track.stop();
        }
        let _ = self.video.pause();
        self.video.set_src_object(None);
        self.video.remove();
    }
}

/// Permission-gated browser camera and display-capture backend.
pub(crate) struct BrowserCaptureBackend;

impl BrowserCaptureBackend {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for BrowserCaptureBackend {
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        anyhow::ensure!(
            browser_media_devices().is_ok(),
            "browser mediaDevices is unavailable"
        );
        let (id, name) = match kind {
            CaptureDeviceKind::Screen | CaptureDeviceKind::Window => {
                ("browser-display-picker", "Browser display picker")
            }
            CaptureDeviceKind::Camera => ("browser-camera-picker", "Browser camera picker"),
            CaptureDeviceKind::Microphone | CaptureDeviceKind::SystemAudio => {
                anyhow::bail!(
                    "browser audio capture is provided by kael_audio's asynchronous AudioWorklet API"
                );
            }
        };
        Ok(vec![CaptureDeviceInfo {
            id: id.into(),
            name: name.into(),
            kind,
            is_available: true,
        }])
    }
}

impl CaptureBackend for BrowserCaptureBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        anyhow::ensure!(
            matches!(
                config.kind,
                CaptureDeviceKind::Screen | CaptureDeviceKind::Window | CaptureDeviceKind::Camera
            ),
            "browser capture supports display/window or camera video; use kael_audio for audio"
        );
        Ok(Box::new(BrowserCaptureSession::new(config.kind)))
    }
}

struct BrowserCaptureSession {
    id: u64,
    expected_kind: CaptureDeviceKind,
    state: Arc<AtomicU8>,
    generation: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
    dropped: Arc<AtomicU64>,
    total_latency_ms: Arc<AtomicU64>,
    frame_count: Arc<AtomicU64>,
}

impl BrowserCaptureSession {
    fn new(expected_kind: CaptureDeviceKind) -> Self {
        Self {
            id: NEXT_BROWSER_CAPTURE_ID
                .fetch_add(1, Ordering::Relaxed)
                .max(1),
            expected_kind,
            state: Arc::new(AtomicU8::new(state_code(CaptureSessionState::Idle))),
            generation: Arc::new(AtomicU64::new(0)),
            error: Arc::new(Mutex::new(None)),
            dropped: Arc::new(AtomicU64::new(0)),
            total_latency_ms: Arc::new(AtomicU64::new(0)),
            frame_count: Arc::new(AtomicU64::new(0)),
        }
    }

    fn set_state(&self, state: CaptureSessionState) {
        self.state.store(state_code(state), Ordering::Release);
    }
}

impl CaptureSession for BrowserCaptureSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        anyhow::ensure!(
            matches!(
                self.state(),
                CaptureSessionState::Idle
                    | CaptureSessionState::Stopped
                    | CaptureSessionState::Error
            ),
            "browser capture session is already active"
        );
        validate_browser_capture_config(self.expected_kind, &config)?;
        cleanup_runtime(self.id);
        *self.error.lock() = None;
        self.dropped.store(0, Ordering::Relaxed);
        self.total_latency_ms.store(0, Ordering::Relaxed);
        self.frame_count.store(0, Ordering::Relaxed);

        // Calling getDisplayMedia/getUserMedia before the first await preserves
        // the transient user activation of the click/key event that invoked start.
        let promise = capture_promise(&config).inspect_err(|error| {
            set_async_error(&self.state, &self.error, format!("{error:#}"));
        })?;
        let generation = self
            .generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.set_state(CaptureSessionState::Starting);

        let id = self.id;
        let state = Arc::clone(&self.state);
        let generations = Arc::clone(&self.generation);
        let error = Arc::clone(&self.error);
        let dropped = Arc::clone(&self.dropped);
        let total_latency_ms = Arc::clone(&self.total_latency_ms);
        let frame_count = Arc::clone(&self.frame_count);
        spawn_local(async move {
            let result = async {
                let value = JsFuture::from(promise).await.map_err(js_error)?;
                let stream = value
                    .dyn_into::<MediaStream>()
                    .map_err(|_| anyhow!("browser capture picker did not return a MediaStream"))?;
                if generations.load(Ordering::Acquire) != generation
                    || load_state(&state) != CaptureSessionState::Starting
                {
                    stop_stream(&stream);
                    return Ok(());
                }
                initialize_runtime(
                    id,
                    config,
                    callback,
                    stream,
                    Arc::clone(&state),
                    Arc::clone(&error),
                    dropped,
                    total_latency_ms,
                    frame_count,
                )
                .await
            }
            .await;

            if let Err(capture_error) = result
                && generations.load(Ordering::Acquire) == generation
                && load_state(&state) == CaptureSessionState::Starting
            {
                set_async_error(&state, &error, format!("{capture_error:#}"));
                cleanup_runtime(id);
            }
        });
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.state() == CaptureSessionState::Running,
            "browser capture can only pause while running"
        );
        self.set_state(CaptureSessionState::Paused);
        pause_runtime(self.id)?;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.state() == CaptureSessionState::Paused,
            "browser capture can only resume while paused"
        );
        self.set_state(CaptureSessionState::Running);
        resume_runtime(self.id, Arc::clone(&self.state), Arc::clone(&self.error))?;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.set_state(CaptureSessionState::Stopped);
        cleanup_runtime(self.id);
        Ok(())
    }

    fn state(&self) -> CaptureSessionState {
        load_state(&self.state)
    }

    fn dropped_frame_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn latency_ms(&self) -> u64 {
        let frames = self.frame_count.load(Ordering::Relaxed);
        self.total_latency_ms
            .load(Ordering::Relaxed)
            .checked_div(frames)
            .unwrap_or_default()
    }

    fn last_error(&self) -> Option<String> {
        self.error.lock().clone()
    }
}

impl Drop for BrowserCaptureSession {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.set_state(CaptureSessionState::Stopped);
        cleanup_runtime(self.id);
    }
}

#[allow(clippy::too_many_arguments)]
async fn initialize_runtime(
    id: u64,
    config: CaptureConfig,
    callback: FrameCallback,
    stream: MediaStream,
    state: Arc<AtomicU8>,
    error: Arc<Mutex<Option<String>>>,
    dropped: Arc<AtomicU64>,
    total_latency_ms: Arc<AtomicU64>,
    frame_count: Arc<AtomicU64>,
) -> Result<()> {
    let tracks = media_tracks(stream.get_video_tracks());
    anyhow::ensure!(
        !tracks.is_empty(),
        "browser capture picker returned no video track"
    );
    let document = web_sys::window()
        .and_then(|window| window.document())
        .context("browser Document is unavailable")?;
    let video = document
        .create_element("video")
        .map_err(js_error)?
        .dyn_into::<HtmlVideoElement>()
        .map_err(|_| anyhow!("failed to create browser capture video element"))?;
    video.set_attribute("playsinline", "").map_err(js_error)?;
    video
        .set_attribute("aria-hidden", "true")
        .map_err(js_error)?;
    for (property, value) in [
        ("position", "fixed"),
        ("width", "1px"),
        ("height", "1px"),
        ("left", "-2px"),
        ("top", "-2px"),
        ("opacity", "0"),
        ("pointer-events", "none"),
    ] {
        video
            .style()
            .set_property(property, value)
            .map_err(js_error)?;
    }
    video.set_autoplay(true);
    video.set_muted(true);
    video.set_src_object(Some(&stream));
    document
        .body()
        .context("browser Document body is unavailable")?
        .append_child(&video)
        .map_err(js_error)?;

    let canvas = document
        .create_element("canvas")
        .map_err(js_error)?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| anyhow!("failed to create browser capture canvas"))?;
    let context_options = JsValue::from(js_sys::Object::new());
    js_sys::Reflect::set(
        &context_options,
        &JsValue::from_str("willReadFrequently"),
        &JsValue::TRUE,
    )
    .map_err(js_error)?;
    let context = canvas
        .get_context_with_context_options("2d", &context_options)
        .map_err(js_error)?
        .context("browser capture 2D canvas is unavailable")?
        .dyn_into::<CanvasRenderingContext2d>()
        .map_err(|_| anyhow!("browser capture returned an invalid 2D context"))?;

    let play = video.play().map_err(js_error)?;
    if let Err(play_error) = JsFuture::from(play).await {
        stop_stream(&stream);
        video.remove();
        return Err(js_error(play_error).context("browser capture video could not start"));
    }

    let mut ended_callbacks = Vec::with_capacity(tracks.len());
    for track in &tracks {
        let ended_state = Arc::clone(&state);
        let ended = Closure::wrap(Box::new(move |_event: Event| {
            ended_state.store(state_code(CaptureSessionState::Stopped), Ordering::Release);
        }) as Box<dyn FnMut(Event)>);
        track.set_onended(Some(ended.as_ref().unchecked_ref()));
        ended_callbacks.push(ended);
    }

    let frame_callback = Closure::wrap(Box::new(move |timestamp_ms: f64| {
        drive_capture_frame(id, timestamp_ms);
    }) as Box<dyn FnMut(f64)>);
    let target_interval_ms = 1000.0 / config.frame_rate.unwrap_or(60.0);
    let runtime = BrowserCaptureRuntime {
        _stream: stream,
        tracks,
        video,
        canvas,
        context,
        frame_callback,
        _ended_callbacks: ended_callbacks,
        callback,
        state: Arc::clone(&state),
        error: Arc::clone(&error),
        dropped,
        total_latency_ms,
        frame_count,
        frame_request: None,
        target_interval_ms,
        last_frame_ms: None,
        requested_resolution: config.resolution,
    };
    BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        if let Some(previous) = runtimes.borrow_mut().insert(id, runtime) {
            previous.shutdown();
        }
    });
    state.store(state_code(CaptureSessionState::Running), Ordering::Release);
    if let Err(frame_error) = request_next_frame(id) {
        set_async_error(&state, &error, format!("{frame_error:#}"));
        cleanup_runtime(id);
    }
    Ok(())
}

fn drive_capture_frame(id: u64, timestamp_ms: f64) {
    let state = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.borrow_mut();
        let runtime = runtimes.get_mut(&id)?;
        runtime.frame_request = None;
        Some(Arc::clone(&runtime.state))
    });
    let Some(state) = state else { return };
    match load_state(&state) {
        CaptureSessionState::Paused => return,
        CaptureSessionState::Running => {}
        _ => {
            cleanup_runtime(id);
            return;
        }
    }

    let started_ms = browser_now_ms();
    let delivery = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.borrow_mut();
        let runtime = runtimes.get_mut(&id)?;
        Some(capture_frame(runtime, timestamp_ms))
    });
    match delivery {
        Some(Ok(Some(delivery))) => {
            (delivery.callback)(delivery.frame);
            let elapsed = (browser_now_ms() - started_ms).max(0.0).round() as u64;
            delivery
                .total_latency_ms
                .fetch_add(elapsed, Ordering::Relaxed);
            delivery.frame_count.fetch_add(1, Ordering::Relaxed);
        }
        Some(Err(frame_error)) => {
            let handles = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
                runtimes
                    .borrow()
                    .get(&id)
                    .map(|runtime| (Arc::clone(&runtime.state), Arc::clone(&runtime.error)))
            });
            if let Some((state, error)) = handles {
                set_async_error(&state, &error, format!("{frame_error:#}"));
            }
            cleanup_runtime(id);
            return;
        }
        Some(Ok(None)) | None => {}
    }

    match load_state(&state) {
        CaptureSessionState::Running => {
            if let Err(frame_error) = request_next_frame(id) {
                let error = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
                    runtimes
                        .borrow()
                        .get(&id)
                        .map(|runtime| Arc::clone(&runtime.error))
                });
                if let Some(error) = error {
                    set_async_error(&state, &error, format!("{frame_error:#}"));
                }
                cleanup_runtime(id);
            }
        }
        CaptureSessionState::Paused => {}
        _ => cleanup_runtime(id),
    }
}

struct FrameDelivery {
    callback: FrameCallback,
    frame: CaptureFrame,
    total_latency_ms: Arc<AtomicU64>,
    frame_count: Arc<AtomicU64>,
}

fn capture_frame(
    runtime: &mut BrowserCaptureRuntime,
    timestamp_ms: f64,
) -> Result<Option<FrameDelivery>> {
    let source_width = runtime.video.video_width();
    let source_height = runtime.video.video_height();
    if source_width == 0 || source_height == 0 {
        runtime.dropped.fetch_add(1, Ordering::Relaxed);
        return Ok(None);
    }
    let (width, height) =
        bounded_frame_dimensions(source_width, source_height, runtime.requested_resolution)?;
    let effective_interval_ms = runtime.target_interval_ms.max(
        1_000.0 * (f64::from(width) * f64::from(height)) / MAX_BROWSER_CAPTURE_PIXELS_PER_SECOND,
    );
    if runtime
        .last_frame_ms
        .is_some_and(|last| timestamp_ms - last + f64::EPSILON < effective_interval_ms)
    {
        return Ok(None);
    }
    runtime.last_frame_ms = Some(timestamp_ms);

    if runtime.canvas.width() != width {
        runtime.canvas.set_width(width);
    }
    if runtime.canvas.height() != height {
        runtime.canvas.set_height(height);
    }
    runtime
        .context
        .draw_image_with_html_video_element_and_dw_and_dh(
            &runtime.video,
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
        )
        .map_err(js_error)?;
    let data = runtime
        .context
        .get_image_data(0.0, 0.0, f64::from(width), f64::from(height))
        .map_err(js_error)?
        .data()
        .0;
    Ok(Some(FrameDelivery {
        callback: Arc::clone(&runtime.callback),
        frame: CaptureFrame::Video {
            width,
            height,
            format: PixelFormat::Rgba32,
            data: Arc::new(data),
            timestamp_ms: finite_timestamp_ms(timestamp_ms),
        },
        total_latency_ms: Arc::clone(&runtime.total_latency_ms),
        frame_count: Arc::clone(&runtime.frame_count),
    }))
}

fn request_next_frame(id: u64) -> Result<()> {
    let function = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        runtimes.borrow().get(&id).map(|runtime| {
            runtime
                .frame_callback
                .as_ref()
                .unchecked_ref::<Function>()
                .clone()
        })
    });
    let Some(function) = function else {
        return Ok(());
    };
    let request = web_sys::window()
        .context("browser Window is unavailable")?
        .request_animation_frame(&function)
        .map_err(js_error)?;
    BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        if let Some(runtime) = runtimes.borrow_mut().get_mut(&id) {
            runtime.frame_request = Some(request);
        }
    });
    Ok(())
}

fn pause_runtime(id: u64) -> Result<()> {
    let (video, frame_request) = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.borrow_mut();
        let runtime = runtimes
            .get_mut(&id)
            .ok_or_else(|| anyhow!("browser capture runtime is unavailable"))?;
        Ok::<_, anyhow::Error>((runtime.video.clone(), runtime.frame_request.take()))
    })?;
    if let Some(request) = frame_request {
        web_sys::window()
            .context("browser Window is unavailable")?
            .cancel_animation_frame(request)
            .map_err(js_error)?;
    }
    video.pause().map_err(js_error)
}

fn resume_runtime(id: u64, state: Arc<AtomicU8>, error: Arc<Mutex<Option<String>>>) -> Result<()> {
    let video = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| {
        runtimes
            .borrow()
            .get(&id)
            .map(|runtime| runtime.video.clone())
            .ok_or_else(|| anyhow!("browser capture runtime is unavailable"))
    })?;
    let play = video.play().map_err(js_error)?;
    spawn_local(async move {
        if let Err(play_error) = JsFuture::from(play).await {
            set_async_error(&state, &error, format!("{:#}", js_error(play_error)));
            cleanup_runtime(id);
        }
    });
    request_next_frame(id)
}

fn cleanup_runtime(id: u64) {
    let runtime = BROWSER_CAPTURE_RUNTIMES.with(|runtimes| runtimes.borrow_mut().remove(&id));
    if let Some(runtime) = runtime {
        runtime.shutdown();
    }
}

fn capture_promise(config: &CaptureConfig) -> Result<Promise> {
    let media_devices = browser_media_devices()?;
    let constraints = MediaStreamConstraints::new();
    constraints.set_video_bool(true);
    constraints.set_audio_bool(false);
    match config.kind {
        CaptureDeviceKind::Screen | CaptureDeviceKind::Window => {
            media_devices.get_display_media().map_err(js_error)
        }
        CaptureDeviceKind::Camera => media_devices
            .get_user_media_with_constraints(&constraints)
            .map_err(js_error),
        CaptureDeviceKind::Microphone | CaptureDeviceKind::SystemAudio => {
            anyhow::bail!("browser audio capture is provided by kael_audio")
        }
    }
}

fn browser_media_devices() -> Result<web_sys::MediaDevices> {
    web_sys::window()
        .context("browser Window is unavailable")?
        .navigator()
        .media_devices()
        .map_err(js_error)
}

fn validate_browser_capture_config(
    expected_kind: CaptureDeviceKind,
    config: &CaptureConfig,
) -> Result<()> {
    anyhow::ensure!(
        config.kind == expected_kind,
        "browser capture session kind does not match its backend"
    );
    anyhow::ensure!(
        !config.include_audio,
        "browser display/camera session currently produces video only; use kael_audio for microphone capture"
    );
    if let Some(frame_rate) = config.frame_rate {
        anyhow::ensure!(
            frame_rate.is_finite() && frame_rate > 0.0 && frame_rate <= MAX_BROWSER_CAPTURE_FPS,
            "browser capture frame rate must be within 0..={MAX_BROWSER_CAPTURE_FPS}"
        );
    }
    if let Some((width, height)) = config.resolution {
        anyhow::ensure!(
            width > 0 && height > 0,
            "browser capture dimensions are empty"
        );
        anyhow::ensure!(
            u64::from(width) * u64::from(height) <= MAX_BROWSER_CAPTURE_PIXELS,
            "browser capture resolution exceeds the bounded frame size"
        );
    }
    Ok(())
}

fn bounded_frame_dimensions(
    source_width: u32,
    source_height: u32,
    requested: Option<(u32, u32)>,
) -> Result<(u32, u32)> {
    let (width, height) = requested.unwrap_or((source_width, source_height));
    anyhow::ensure!(width > 0 && height > 0, "browser capture frame is empty");
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= MAX_BROWSER_CAPTURE_PIXELS {
        return Ok((width, height));
    }

    let scale = (MAX_BROWSER_CAPTURE_PIXELS as f64 / pixels as f64).sqrt();
    let bounded_width = (f64::from(width) * scale).floor().max(1.0) as u32;
    let bounded_height = (f64::from(height) * scale).floor().max(1.0) as u32;
    Ok((bounded_width, bounded_height))
}

fn media_tracks(array: js_sys::Array) -> Vec<MediaStreamTrack> {
    array
        .iter()
        .filter_map(|value| value.dyn_into::<MediaStreamTrack>().ok())
        .collect()
}

fn stop_stream(stream: &MediaStream) {
    for track in media_tracks(stream.get_tracks()) {
        track.stop();
    }
}

fn browser_now_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

fn finite_timestamp_ms(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value.min(u64::MAX as f64).round() as u64
    } else {
        0
    }
}

fn set_async_error(state: &Arc<AtomicU8>, error: &Arc<Mutex<Option<String>>>, message: String) {
    *error.lock() = Some(message);
    state.store(state_code(CaptureSessionState::Error), Ordering::Release);
}

fn state_code(state: CaptureSessionState) -> u8 {
    match state {
        CaptureSessionState::Idle => 0,
        CaptureSessionState::Starting => 1,
        CaptureSessionState::Running => 2,
        CaptureSessionState::Paused => 3,
        CaptureSessionState::Stopped => 4,
        CaptureSessionState::Error => 5,
    }
}

fn load_state(state: &AtomicU8) -> CaptureSessionState {
    match state.load(Ordering::Acquire) {
        0 => CaptureSessionState::Idle,
        1 => CaptureSessionState::Starting,
        2 => CaptureSessionState::Running,
        3 => CaptureSessionState::Paused,
        4 => CaptureSessionState::Stopped,
        _ => CaptureSessionState::Error,
    }
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}
