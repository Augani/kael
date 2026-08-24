//! Permission-safe browser microphone capture through a credit-bounded AudioWorklet.

use std::{
    cell::RefCell,
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use anyhow::Result;
use js_sys::{Array, Float32Array, Object, Reflect};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    AudioContext, AudioContextOptions, AudioContextState, AudioWorkletNode,
    AudioWorkletNodeOptions, Event, MediaStream, MediaStreamAudioSourceNode,
    MediaStreamConstraints, MediaStreamTrack, MessageEvent, MessagePort,
};

use crate::AudioInputDevice;
use crate::browser_audio::{
    BrowserAudioError, BrowserAudioErrorKind, BrowserAudioEvent, PendingAudioContext,
    classify_js_error, install_worklet, validate_worklet_bounds,
};
use crate::browser_audio_protocol::{CaptureDeliveryError, CaptureDeliveryState};

const ASYNC_CAPTURE_MESSAGE: &str =
    "browser microphone capture requires AudioInputStream::new_async";
const CAPTURE_EVENT_CAPACITY: usize = 64;
const MAX_CAPTURE_TRACKS: u32 = 8;
const MIN_BROWSER_SAMPLE_RATE: u32 = 8_000;
const MAX_BROWSER_SAMPLE_RATE: u32 = 192_000;

/// The normalized, interleaved sample format delivered by an input stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputConfig {
    /// Samples per second for each channel after browser context conversion.
    pub sample_rate: u32,
    /// Interleaved channel count.
    pub channels: u16,
}

/// Checked browser microphone capture and signal-processing bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserAudioCaptureConfig {
    channels: u16,
    chunk_frames: usize,
    pending_chunks: usize,
    echo_cancellation: bool,
    noise_suppression: bool,
    auto_gain_control: bool,
}

impl BrowserAudioCaptureConfig {
    /// Create checked capture bounds.
    ///
    /// Chunks must be 128..=4096 frames in 128-frame increments. Channels are
    /// limited to 1..=8 and pending delivery credits to 2..=32.
    pub fn new(
        channels: u16,
        chunk_frames: usize,
        pending_chunks: usize,
    ) -> std::result::Result<Self, BrowserAudioError> {
        validate_worklet_bounds(channels, chunk_frames, pending_chunks)?;
        Ok(Self {
            channels,
            chunk_frames,
            pending_chunks,
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain_control: true,
        })
    }

    /// Request browser voice-processing constraints.
    pub fn with_signal_processing(
        mut self,
        echo_cancellation: bool,
        noise_suppression: bool,
        auto_gain_control: bool,
    ) -> Self {
        self.echo_cancellation = echo_cancellation;
        self.noise_suppression = noise_suppression;
        self.auto_gain_control = auto_gain_control;
        self
    }

    /// Normalized callback channels.
    pub fn channels(self) -> u16 {
        self.channels
    }

    /// Frames in one callback slice.
    pub fn chunk_frames(self) -> usize {
        self.chunk_frames
    }

    /// Maximum transferred callback chunks awaiting main-thread delivery.
    pub fn pending_chunks(self) -> usize {
        self.pending_chunks
    }

    /// Whether echo cancellation is requested.
    pub fn echo_cancellation(self) -> bool {
        self.echo_cancellation
    }

    /// Whether noise suppression is requested.
    pub fn noise_suppression(self) -> bool {
        self.noise_suppression
    }

    /// Whether automatic gain control is requested.
    pub fn auto_gain_control(self) -> bool {
        self.auto_gain_control
    }
}

impl Default for BrowserAudioCaptureConfig {
    fn default() -> Self {
        Self {
            channels: 1,
            chunk_frames: 1_024,
            pending_chunks: 4,
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain_control: true,
        }
    }
}

/// A live browser microphone stream.
///
/// The worklet holds a fixed number of delivery credits. If application code
/// cannot consume callbacks promptly, frames are dropped and reported rather
/// than accumulating an unbounded `MessagePort` queue.
#[must_use = "dropping the stream stops every microphone track"]
pub struct AudioInputStream {
    inner: Rc<CaptureInner>,
    config: AudioInputConfig,
}

type CaptureCallback = Box<dyn FnMut(&[f32], AudioInputConfig)>;

struct CaptureInner {
    state: RefCell<CaptureState>,
}

struct CaptureState {
    context: AudioContext,
    source: MediaStreamAudioSourceNode,
    node: AudioWorkletNode,
    port: MessagePort,
    tracks: Vec<MediaStreamTrack>,
    callback: Option<CaptureCallback>,
    config: AudioInputConfig,
    expected_samples: usize,
    delivery: CaptureDeliveryState,
    last_input_error: Option<String>,
    events: VecDeque<BrowserAudioEvent>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_processor_error: Option<Closure<dyn FnMut(Event)>>,
    on_track_ended: Vec<Closure<dyn FnMut(Event)>>,
    closed: bool,
}

impl AudioInputStream {
    /// Return an explicit error because browser permission cannot be synchronous.
    pub fn new(callback: impl FnMut(&[f32], AudioInputConfig) + Send + 'static) -> Result<Self> {
        drop(callback);
        anyhow::bail!(ASYNC_CAPTURE_MESSAGE)
    }

    /// Return an explicit error because browser permission cannot be synchronous.
    pub fn from_input_device(
        _device: &AudioInputDevice,
        callback: impl FnMut(&[f32], AudioInputConfig) + Send + 'static,
    ) -> Result<Self> {
        drop(callback);
        anyhow::bail!(ASYNC_CAPTURE_MESSAGE)
    }

    /// Request the default microphone and start bounded asynchronous capture.
    pub async fn new_async(
        callback: impl FnMut(&[f32], AudioInputConfig) + 'static,
    ) -> std::result::Result<Self, BrowserAudioError> {
        Self::new_async_with_config(BrowserAudioCaptureConfig::default(), callback).await
    }

    /// Request the default microphone with explicit worklet bounds.
    pub async fn new_async_with_config(
        config: BrowserAudioCaptureConfig,
        callback: impl FnMut(&[f32], AudioInputConfig) + 'static,
    ) -> std::result::Result<Self, BrowserAudioError> {
        Self::open_async(config, None, Box::new(callback)).await
    }

    /// Request a selected browser microphone asynchronously.
    pub async fn from_input_device_async(
        device: &AudioInputDevice,
        callback: impl FnMut(&[f32], AudioInputConfig) + 'static,
    ) -> std::result::Result<Self, BrowserAudioError> {
        Self::open_async(
            BrowserAudioCaptureConfig::default(),
            Some(device.device_id()),
            Box::new(callback),
        )
        .await
    }

    /// Request a selected browser microphone with explicit worklet bounds.
    pub async fn from_input_device_async_with_config(
        device: &AudioInputDevice,
        config: BrowserAudioCaptureConfig,
        callback: impl FnMut(&[f32], AudioInputConfig) + 'static,
    ) -> std::result::Result<Self, BrowserAudioError> {
        Self::open_async(config, Some(device.device_id()), Box::new(callback)).await
    }

    async fn open_async(
        capture_config: BrowserAudioCaptureConfig,
        device_id: Option<&str>,
        callback: CaptureCallback,
    ) -> std::result::Result<Self, BrowserAudioError> {
        validate_worklet_bounds(
            capture_config.channels,
            capture_config.chunk_frames,
            capture_config.pending_chunks,
        )?;
        let expected_samples = capture_config
            .chunk_frames
            .checked_mul(usize::from(capture_config.channels))
            .ok_or_else(|| BrowserAudioError::new(BrowserAudioErrorKind::InvalidConfiguration))?;
        let media_devices = web_sys::window()
            .ok_or_else(|| BrowserAudioError::new(BrowserAudioErrorKind::ApiUnavailable))?
            .navigator()
            .media_devices()
            .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::ApiUnavailable))?;
        let constraints = media_constraints(capture_config, device_id)?;
        let promise = media_devices
            .get_user_media_with_constraints(&constraints)
            .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::PermissionDenied))?;
        let stream_value = JsFuture::from(promise)
            .await
            .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::PermissionDenied))?;
        let stream = stream_value
            .dyn_into::<MediaStream>()
            .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::DeviceUnavailable))?;
        let pending_tracks = PendingCaptureTracks::new(audio_tracks(&stream)?);

        let context_options = AudioContextOptions::new();
        context_options.set_latency_hint(&JsValue::from_str("interactive"));
        let context = AudioContext::new_with_context_options(&context_options)
            .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::ApiUnavailable))?;
        let pending_context = PendingAudioContext::new(context);
        install_worklet(pending_context.context()).await?;
        let sample_rate = checked_context_sample_rate(pending_context.context().sample_rate())?;
        let source = pending_context
            .context()
            .create_media_stream_source(&stream)
            .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))?;
        let node =
            create_capture_node(pending_context.context(), capture_config).inspect_err(|_| {
                let _ = source.disconnect();
            })?;
        source.connect_with_audio_node(&node).map_err(|_| {
            let _ = source.disconnect();
            let _ = node.disconnect();
            BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable)
        })?;
        node.connect_with_audio_node(&pending_context.context().destination())
            .map_err(|_| {
                let _ = source.disconnect();
                let _ = node.disconnect();
                BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable)
            })?;
        let port = match node.port() {
            Ok(port) => port,
            Err(_) => {
                let _ = source.disconnect();
                let _ = node.disconnect();
                return Err(BrowserAudioError::new(
                    BrowserAudioErrorKind::WorkletUnavailable,
                ));
            }
        };
        let config = AudioInputConfig {
            sample_rate,
            channels: capture_config.channels,
        };
        let context = pending_context.into_inner();
        let tracks = pending_tracks.into_inner();
        let inner = Rc::new(CaptureInner {
            state: RefCell::new(CaptureState {
                context,
                source,
                node,
                port,
                tracks,
                callback: Some(callback),
                config,
                expected_samples,
                delivery: CaptureDeliveryState::default(),
                last_input_error: None,
                events: VecDeque::with_capacity(CAPTURE_EVENT_CAPACITY),
                on_message: None,
                on_processor_error: None,
                on_track_ended: Vec::new(),
                closed: false,
            }),
        });
        install_capture_handlers(&inner);
        let resume = match inner.state.borrow().context.resume() {
            Ok(resume) => resume,
            Err(_) => {
                shutdown_capture_detached(Rc::clone(&inner));
                return Err(BrowserAudioError::new(
                    BrowserAudioErrorKind::UserActivationRequired,
                ));
            }
        };
        if let Err(error) = JsFuture::from(resume).await {
            let classified =
                classify_js_error(&error, BrowserAudioErrorKind::UserActivationRequired);
            let error = if classified.kind() == BrowserAudioErrorKind::PermissionDenied {
                BrowserAudioError::new(BrowserAudioErrorKind::UserActivationRequired)
            } else {
                classified
            };
            shutdown_capture_detached(Rc::clone(&inner));
            return Err(error);
        }
        push_capture_event(
            &mut inner.state.borrow_mut().events,
            BrowserAudioEvent::Running,
        );
        Ok(Self { inner, config })
    }

    /// Return the actual normalized callback format.
    pub fn config(&self) -> AudioInputConfig {
        self.config
    }

    /// Whether capture is currently rendering through the worklet.
    pub fn is_running(&self) -> bool {
        let state = self.inner.state.borrow();
        !state.closed && state.context.state() == AudioContextState::Running
    }

    /// Cumulative frames dropped by credit-based backpressure.
    pub fn dropped_frames(&self) -> u64 {
        self.inner.state.borrow().delivery.dropped_frames()
    }

    /// Poll one bounded capture lifecycle or pressure event.
    pub fn poll_event(&self) -> Option<BrowserAudioEvent> {
        self.inner.state.borrow_mut().events.pop_front()
    }

    /// Return and clear the most recent asynchronous capture error.
    pub fn take_input_error(&self) -> Option<String> {
        self.inner.state.borrow_mut().last_input_error.take()
    }

    /// Stop every microphone track and await `AudioContext.close()`.
    pub async fn close_async(self) -> std::result::Result<(), BrowserAudioError> {
        if let Some(promise) = self.inner.shutdown() {
            JsFuture::from(promise)
                .await
                .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::Closed))?;
        }
        Ok(())
    }
}

impl CaptureInner {
    fn shutdown(&self) -> Option<js_sys::Promise> {
        let mut state = self.state.borrow_mut();
        if state.closed {
            return None;
        }
        state.closed = true;
        state.port.set_onmessage(None);
        state.node.set_onprocessorerror(None);
        for track in &state.tracks {
            track.set_onended(None);
            track.stop();
        }
        let _ = post_capture_control(&state.port, "stop", None);
        let _ = state.source.disconnect();
        let _ = state.node.disconnect();
        state.port.close();
        let callback = state.callback.take();
        state.on_message.take();
        state.on_processor_error.take();
        state.on_track_ended.clear();
        push_capture_event(&mut state.events, BrowserAudioEvent::Closed);
        let promise = state.context.close().ok();
        drop(state);
        if let Some(callback) = callback {
            safe_drop_callback(callback);
        }
        promise
    }
}

impl Drop for CaptureInner {
    fn drop(&mut self) {
        if let Some(promise) = self.shutdown() {
            spawn_local(async move {
                let _ = JsFuture::from(promise).await;
            });
        }
    }
}

fn media_constraints(
    config: BrowserAudioCaptureConfig,
    device_id: Option<&str>,
) -> std::result::Result<MediaStreamConstraints, BrowserAudioError> {
    let audio = Object::new();
    set_object_property(
        &audio,
        "channelCount",
        &JsValue::from(u32::from(config.channels)),
    )?;
    set_object_property(
        &audio,
        "echoCancellation",
        &JsValue::from_bool(config.echo_cancellation),
    )?;
    set_object_property(
        &audio,
        "noiseSuppression",
        &JsValue::from_bool(config.noise_suppression),
    )?;
    set_object_property(
        &audio,
        "autoGainControl",
        &JsValue::from_bool(config.auto_gain_control),
    )?;
    if let Some(device_id) = device_id {
        let exact = Object::new();
        set_object_property(&exact, "exact", &JsValue::from_str(device_id))?;
        set_object_property(&audio, "deviceId", exact.as_ref())?;
    }
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(audio.as_ref());
    constraints.set_video_bool(false);
    Ok(constraints)
}

fn audio_tracks(
    stream: &MediaStream,
) -> std::result::Result<Vec<MediaStreamTrack>, BrowserAudioError> {
    let values = stream.get_audio_tracks();
    if values.length() == 0 {
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::DeviceUnavailable,
        ));
    }
    if values.length() > MAX_CAPTURE_TRACKS {
        for value in values.iter() {
            if let Ok(track) = value.dyn_into::<MediaStreamTrack>() {
                track.stop();
            }
        }
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::InvalidConfiguration,
        ));
    }
    let mut tracks = Vec::new();
    for index in 0..values.length() {
        if let Ok(track) = values.get(index).dyn_into::<MediaStreamTrack>() {
            tracks.push(track);
        }
    }
    if tracks.is_empty() {
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::DeviceUnavailable,
        ));
    }
    Ok(tracks)
}

fn create_capture_node(
    context: &AudioContext,
    config: BrowserAudioCaptureConfig,
) -> std::result::Result<AudioWorkletNode, BrowserAudioError> {
    let options = AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(1);
    options.set_number_of_outputs(1);
    options.set_channel_count(u32::from(config.channels));
    let output_channels = Array::new();
    output_channels.push(&JsValue::from(1_u32));
    options.set_output_channel_count(output_channels.as_ref());
    let processor = Object::new();
    set_object_property(
        &processor,
        "channels",
        &JsValue::from(u32::from(config.channels)),
    )?;
    set_object_property(
        &processor,
        "chunkFrames",
        &JsValue::from(config.chunk_frames as u32),
    )?;
    set_object_property(
        &processor,
        "pendingChunks",
        &JsValue::from(config.pending_chunks as u32),
    )?;
    options.set_processor_options(Some(&processor));
    AudioWorkletNode::new_with_options(context, "kael-capture-v1", &options)
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))
}

fn install_capture_handlers(inner: &Rc<CaptureInner>) {
    let weak = Rc::downgrade(inner);
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event| {
        if let Some(inner) = weak.upgrade() {
            handle_capture_message(&inner, event);
        }
    });
    let weak = Rc::downgrade(inner);
    let on_processor_error = Closure::<dyn FnMut(Event)>::new(move |_| {
        if let Some(inner) = weak.upgrade() {
            {
                let mut state = inner.state.borrow_mut();
                state.last_input_error =
                    Some("browser capture worklet processor failed".to_string());
                push_capture_event(&mut state.events, BrowserAudioEvent::ProcessorError);
            }
            shutdown_capture_detached(inner);
        }
    });
    let tracks = inner.state.borrow().tracks.clone();
    let mut ended_handlers = Vec::with_capacity(tracks.len());
    for track in &tracks {
        let weak = Rc::downgrade(inner);
        let on_ended = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Some(inner) = weak.upgrade() {
                let mut state = inner.state.borrow_mut();
                if !state.closed {
                    state.last_input_error = Some("browser microphone track ended".to_string());
                    push_capture_event(&mut state.events, BrowserAudioEvent::CaptureEnded);
                }
                drop(state);
                shutdown_capture_detached(inner);
            }
        });
        track.set_onended(Some(on_ended.as_ref().unchecked_ref()));
        ended_handlers.push(on_ended);
    }
    let mut state = inner.state.borrow_mut();
    state
        .port
        .set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    state.port.start();
    state
        .node
        .set_onprocessorerror(Some(on_processor_error.as_ref().unchecked_ref()));
    state.on_message = Some(on_message);
    state.on_processor_error = Some(on_processor_error);
    state.on_track_ended = ended_handlers;
}

fn handle_capture_message(inner: &Rc<CaptureInner>, event: MessageEvent) {
    if object_string(&event.data(), "type").as_deref() != Some("capture") {
        return;
    }
    let samples = Reflect::get(&event.data(), &JsValue::from_str("samples"))
        .ok()
        .and_then(|value| value.dyn_into::<Float32Array>().ok());
    let mut state = inner.state.borrow_mut();
    if state.closed {
        return;
    }
    let Some(samples) = samples else {
        let callback = disable_capture_callback(
            &mut state,
            "browser capture delivered an invalid sample chunk",
        );
        drop(state);
        if let Some(callback) = callback {
            safe_drop_callback(callback);
        }
        shutdown_capture_detached(Rc::clone(inner));
        return;
    };
    let expected_samples = state.expected_samples;
    let delivery = state.delivery.accept(
        samples.length() as usize,
        expected_samples,
        object_number(&event.data(), "sequence"),
        object_number(&event.data(), "dropped"),
    );
    let newly_dropped = match delivery {
        Ok(newly_dropped) => newly_dropped,
        Err(error) => {
            let message = match error {
                CaptureDeliveryError::InvalidSampleCount => {
                    "browser capture sample chunk violated its bound"
                }
                CaptureDeliveryError::MissingSequence | CaptureDeliveryError::OutOfOrder => {
                    "browser capture sample sequence was not ordered"
                }
                CaptureDeliveryError::MissingDropCounter
                | CaptureDeliveryError::RegressedDropCounter => {
                    "browser capture pressure counter was invalid"
                }
            };
            let callback = disable_capture_callback(&mut state, message);
            drop(state);
            if let Some(callback) = callback {
                safe_drop_callback(callback);
            }
            shutdown_capture_detached(Rc::clone(inner));
            return;
        }
    };
    if let Some(dropped) = newly_dropped {
        push_capture_event(
            &mut state.events,
            BrowserAudioEvent::CaptureOverflow {
                total_frames: dropped,
            },
        );
    }
    let samples = samples.to_vec();
    let config = state.config;
    let callback_ok = state.callback.as_mut().is_some_and(|callback| {
        catch_unwind(AssertUnwindSafe(|| callback(&samples, config))).is_ok()
    });
    if !callback_ok {
        let callback = disable_capture_callback(
            &mut state,
            "browser audio input callback panicked; capture callback disabled",
        );
        drop(state);
        if let Some(callback) = callback {
            safe_drop_callback(callback);
        }
        shutdown_capture_detached(Rc::clone(inner));
        return;
    }
    if post_capture_control(&state.port, "credit", Some(1)).is_err() {
        state.last_input_error =
            Some("browser capture worklet rejected delivery credit".to_string());
        push_capture_event(&mut state.events, BrowserAudioEvent::ProcessorError);
        drop(state);
        shutdown_capture_detached(Rc::clone(inner));
    }
}

fn disable_capture_callback(state: &mut CaptureState, message: &str) -> Option<CaptureCallback> {
    let callback = state.callback.take();
    state.last_input_error = Some(message.to_string());
    push_capture_event(
        &mut state.events,
        BrowserAudioEvent::CaptureCallbackDisabled,
    );
    let _ = post_capture_control(&state.port, "stop", None);
    callback
}

fn post_capture_control(
    port: &MessagePort,
    message_type: &str,
    count: Option<u32>,
) -> std::result::Result<(), BrowserAudioError> {
    let message = Object::new();
    set_object_property(&message, "type", &JsValue::from_str(message_type))?;
    if let Some(count) = count {
        set_object_property(&message, "count", &JsValue::from(count))?;
    }
    port.post_message(message.as_ref())
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::Processor))
}

fn set_object_property(
    object: &Object,
    name: &str,
    value: &JsValue,
) -> std::result::Result<(), BrowserAudioError> {
    Reflect::set(object, &JsValue::from_str(name), value)
        .map(|_| ())
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::InvalidConfiguration))
}

fn object_string(value: &JsValue, name: &str) -> Option<String> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_string())
}

fn object_number(value: &JsValue, name: &str) -> Option<f64> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite())
}

fn checked_context_sample_rate(rate: f32) -> std::result::Result<u32, BrowserAudioError> {
    if !rate.is_finite() {
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::InvalidConfiguration,
        ));
    }
    let rounded = rate.round();
    if rounded < MIN_BROWSER_SAMPLE_RATE as f32 || rounded > MAX_BROWSER_SAMPLE_RATE as f32 {
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::InvalidConfiguration,
        ));
    }
    Ok(rounded as u32)
}

fn stop_tracks(tracks: &[MediaStreamTrack]) {
    for track in tracks {
        track.stop();
    }
}

fn safe_drop_callback(callback: CaptureCallback) {
    if catch_unwind(AssertUnwindSafe(|| drop(callback))).is_err() {
        log::error!("browser audio input callback destructor panicked during cleanup");
    }
}

struct PendingCaptureTracks {
    tracks: Option<Vec<MediaStreamTrack>>,
}

impl PendingCaptureTracks {
    fn new(tracks: Vec<MediaStreamTrack>) -> Self {
        Self {
            tracks: Some(tracks),
        }
    }

    fn into_inner(mut self) -> Vec<MediaStreamTrack> {
        self.tracks
            .take()
            .expect("pending browser microphone tracks must exist")
    }
}

impl Drop for PendingCaptureTracks {
    fn drop(&mut self) {
        if let Some(tracks) = self.tracks.take() {
            stop_tracks(&tracks);
        }
    }
}

fn push_capture_event(events: &mut VecDeque<BrowserAudioEvent>, event: BrowserAudioEvent) {
    if events.len() == CAPTURE_EVENT_CAPACITY {
        events.pop_front();
    }
    events.push_back(event);
}

fn shutdown_capture_detached(inner: Rc<CaptureInner>) {
    spawn_local(async move {
        if let Some(promise) = inner.shutdown() {
            let _ = JsFuture::from(promise).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_config_is_strictly_bounded() {
        assert!(BrowserAudioCaptureConfig::new(1, 1_024, 4).is_ok());
        assert!(BrowserAudioCaptureConfig::new(9, 1_024, 4).is_err());
        assert!(BrowserAudioCaptureConfig::new(1, 1_000, 4).is_err());
    }

    #[test]
    fn media_constraints_keep_processing_choices_explicit() {
        let config = BrowserAudioCaptureConfig::new(2, 256, 2)
            .unwrap()
            .with_signal_processing(false, false, false);
        let constraints = media_constraints(config, Some("private-id")).unwrap();
        assert!(constraints.get_audio().is_object());
        assert!(!constraints.get_video().as_bool().unwrap_or(true));
    }
}
