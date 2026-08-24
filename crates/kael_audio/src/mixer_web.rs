//! Browser audio mixing and bounded live AudioWorklet output.
//!
//! Offline mixing, DSP sources, resampling, and spatial wrappers remain available
//! in browser builds. [`AudioEngine::new_async`] connects the same mixer to an
//! event-driven AudioWorklet bridge with bounded transferred sample batches.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::{Rc, Weak},
    time::Duration,
};

use anyhow::{Context as _, Result};
use js_sys::{Array, Float32Array, Object, Reflect};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    AudioContext, AudioContextOptions, AudioContextState, AudioWorkletNode,
    AudioWorkletNodeOptions, Event, MessageEvent, MessagePort,
};

use crate::browser_audio::{
    BrowserAudioError, BrowserAudioErrorKind, BrowserAudioEvent, PendingAudioContext,
    classify_js_error, install_worklet, validate_worklet_bounds,
};
use crate::browser_audio_protocol::bounded_output_request;

const MAX_OFFLINE_SAMPLES: usize = 64 * 1024 * 1024;
const MAX_MIXER_VOICES: usize = 4 * 1024;
const MAX_ACTIVE_VOICES: usize = 1024;
const INITIAL_VOICE_CAPACITY: usize = 64;
const ENGINE_EVENT_CAPACITY: usize = 64;
const MIN_BROWSER_SAMPLE_RATE: u32 = 8_000;
const MAX_BROWSER_SAMPLE_RATE: u32 = 192_000;
const LIVE_ENGINE_ASYNC_MESSAGE: &str =
    "browser live sample mixing requires AudioEngine::new_async";

/// Identifier for a voice playing in a [`Mixer`].
pub type VoiceId = u64;

/// Monotonic frame clock used by the device-free browser mixer.
#[derive(Clone)]
pub struct AudioClock {
    frames: Rc<Cell<u64>>,
    sample_rate: u32,
}

impl AudioClock {
    /// Create a clock for the given output sample rate (per channel, in Hz).
    pub fn new(sample_rate: u32) -> Self {
        Self {
            frames: Rc::new(Cell::new(0)),
            sample_rate: sample_rate.max(1),
        }
    }

    /// The output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Total frames processed since the last reset.
    pub fn frames(&self) -> u64 {
        self.frames.get()
    }

    /// Playback position in seconds (processed frames / sample rate).
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// Playback position as a [`Duration`].
    pub fn duration(&self) -> Duration {
        let frames = self.frames();
        let sample_rate = u64::from(self.sample_rate);
        let seconds = frames / sample_rate;
        let nanos = ((frames % sample_rate) * 1_000_000_000) / sample_rate;
        Duration::new(seconds, u32::try_from(nanos).unwrap_or(999_999_999))
    }

    /// Reset the frame counter to zero.
    pub fn reset(&self) {
        self.frames.set(0);
    }

    fn advance(&self, frames: u64) {
        self.frames.set(self.frames.get().saturating_add(frames));
    }
}

/// A streaming source of interleaved `f32` samples at the mixer's rate.
pub trait SampleSource: Send {
    /// Fill `out` with interleaved samples and return the number of frames written.
    fn fill(&mut self, out: &mut [f32], channels: u16) -> usize;
}

/// A finite (optionally looping) buffer of interleaved samples.
pub struct BufferSource {
    samples: Vec<f32>,
    source_channels: u16,
    looping: bool,
    cursor: usize,
}

impl BufferSource {
    /// Wrap interleaved `samples` carrying `channels` per frame.
    pub fn new(samples: Vec<f32>, channels: u16) -> Self {
        Self {
            samples,
            source_channels: channels.max(1),
            looping: false,
            cursor: 0,
        }
    }

    /// Loop the buffer instead of ending when it is exhausted.
    pub fn looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    fn next_frame(&mut self, channels: u16, frame: &mut [f32]) -> bool {
        let source_channels = usize::from(self.source_channels);
        if self
            .cursor
            .checked_add(source_channels)
            .is_none_or(|end| end > self.samples.len())
        {
            if self.looping && self.samples.len() >= source_channels {
                self.cursor = 0;
            } else {
                return false;
            }
        }
        let source = &self.samples[self.cursor..self.cursor + source_channels];
        for (channel, output) in frame.iter_mut().enumerate().take(usize::from(channels)) {
            *output = if source_channels == 1 {
                source[0]
            } else {
                source[channel.min(source_channels - 1)]
            };
        }
        self.cursor += source_channels;
        true
    }
}

impl SampleSource for BufferSource {
    fn fill(&mut self, output: &mut [f32], channels: u16) -> usize {
        let channels = usize::from(channels.max(1));
        let frames = output.len() / channels;
        let mut written = 0;
        for frame_index in 0..frames {
            let start = frame_index * channels;
            if !self.next_frame(channels as u16, &mut output[start..start + channels]) {
                break;
            }
            written += 1;
        }
        written
    }
}

/// A sine-wave generator, primarily for tests and tone processing.
pub struct SineSource {
    phase: f32,
    phase_increment: f32,
    amplitude: f32,
    remaining_frames: Option<usize>,
}

impl SineSource {
    /// Create an endless sine generator at `frequency` Hz for the given rate.
    pub fn new(frequency: f32, sample_rate: u32, amplitude: f32) -> Self {
        let sample_rate = sample_rate.max(1) as f32;
        let frequency = if frequency.is_finite() {
            frequency.max(0.0)
        } else {
            0.0
        };
        let amplitude = if amplitude.is_finite() {
            amplitude
        } else {
            0.0
        };
        Self {
            phase: 0.0,
            phase_increment: std::f32::consts::TAU * frequency / sample_rate,
            amplitude,
            remaining_frames: None,
        }
    }

    /// Limit the generator to `frames` total frames.
    pub fn with_frames(mut self, frames: usize) -> Self {
        self.remaining_frames = Some(frames);
        self
    }
}

impl SampleSource for SineSource {
    fn fill(&mut self, output: &mut [f32], channels: u16) -> usize {
        let channels = usize::from(channels.max(1));
        let available_frames = output.len() / channels;
        let frames = self.remaining_frames.map_or(available_frames, |remaining| {
            available_frames.min(remaining)
        });
        for frame_index in 0..frames {
            let sample = self.amplitude * self.phase.sin();
            self.phase = (self.phase + self.phase_increment).rem_euclid(std::f32::consts::TAU);
            let start = frame_index * channels;
            output[start..start + channels].fill(sample);
        }
        if let Some(remaining) = self.remaining_frames.as_mut() {
            *remaining -= frames;
        }
        frames
    }
}

struct Voice {
    id: VoiceId,
    source: Box<dyn SampleSource>,
    gain: f32,
    ended: bool,
}

/// A deterministic mixing graph that is available in browser builds.
pub struct Mixer {
    sample_rate: u32,
    channels: u16,
    master_gain: f32,
    voices: Vec<Voice>,
    clock: AudioClock,
    scratch: Vec<f32>,
}

impl Mixer {
    /// Create a mixer for the given output rate and channel count.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let sample_rate = sample_rate.max(1);
        Self {
            sample_rate,
            channels: channels.max(1),
            master_gain: 1.0,
            voices: Vec::with_capacity(INITIAL_VOICE_CAPACITY),
            clock: AudioClock::new(sample_rate),
            scratch: Vec::new(),
        }
    }

    /// The output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The output channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// A clone of the shared processing clock.
    pub fn clock(&self) -> AudioClock {
        self.clock.clone()
    }

    /// The current master gain.
    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    /// Set the master gain applied to the summed mix.
    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = finite_gain(gain);
    }

    /// Insert a voice with an explicit id, replacing a voice with the same id.
    pub fn insert_voice(
        &mut self,
        id: VoiceId,
        source: Box<dyn SampleSource>,
        gain: f32,
    ) -> Result<()> {
        self.remove_voice(id);
        if self.voices.len() >= MAX_MIXER_VOICES {
            safe_drop_source(source);
            anyhow::bail!("audio mixer exceeds the {MAX_MIXER_VOICES}-voice limit");
        }
        if let Err(error) = self.voices.try_reserve(1) {
            safe_drop_source(source);
            return Err(error).context("failed to reserve an audio mixer voice");
        }
        self.voices.push(Voice {
            id,
            source,
            gain: finite_gain(gain),
            ended: false,
        });
        Ok(())
    }

    /// Set a voice's gain. Returns `false` if no such voice exists.
    pub fn set_voice_gain(&mut self, id: VoiceId, gain: f32) -> bool {
        let Some(voice) = self
            .voices
            .iter_mut()
            .find(|voice| voice.id == id && !voice.ended)
        else {
            return false;
        };
        voice.gain = finite_gain(gain);
        true
    }

    /// Remove a voice. Returns `false` if no such voice exists.
    pub fn remove_voice(&mut self, id: VoiceId) -> bool {
        let Some(index) = self
            .voices
            .iter()
            .position(|voice| voice.id == id && !voice.ended)
        else {
            return false;
        };
        let voice = self.voices.swap_remove(index);
        safe_drop_source(voice.source);
        true
    }

    /// Number of currently active voices.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| !voice.ended).count()
    }

    /// Sum active voices into `output` and advance the processing clock.
    pub fn process(&mut self, output: &mut [f32]) {
        self.process_inner(output, true);
    }

    fn process_inner(&mut self, output: &mut [f32], advance_clock: bool) {
        self.retire_ended();
        output.fill(0.0);
        let channels = usize::from(self.channels);
        let frames = output.len() / channels;
        if frames == 0 {
            return;
        }
        let needed = frames * channels;
        if self.scratch.len() < needed {
            self.scratch.resize(needed, 0.0);
        }
        for voice in &mut self.voices {
            self.scratch[..needed].fill(0.0);
            let written_frames = catch_unwind(AssertUnwindSafe(|| {
                voice
                    .source
                    .fill(&mut self.scratch[..needed], self.channels)
            }))
            .unwrap_or_else(|_| {
                voice.ended = true;
                0
            })
            .min(frames);
            let written_samples = written_frames.saturating_mul(channels).min(needed);
            for (target, source) in output[..written_samples]
                .iter_mut()
                .zip(&self.scratch[..written_samples])
            {
                if source.is_finite() {
                    let mixed = f64::from(*target) + f64::from(voice.gain) * f64::from(*source);
                    *target = mixed.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
                }
            }
            if written_frames < frames {
                voice.ended = true;
            }
        }
        if (self.master_gain - 1.0).abs() > f32::EPSILON {
            for sample in &mut output[..needed] {
                let scaled = f64::from(*sample) * f64::from(self.master_gain);
                *sample = scaled.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
            }
        }
        if advance_clock {
            self.clock
                .advance(u64::try_from(frames).unwrap_or(u64::MAX));
        }
        self.retire_ended();
    }

    fn retire_ended(&mut self) {
        let mut index = 0;
        while index < self.voices.len() {
            if self.voices[index].ended {
                let voice = self.voices.swap_remove(index);
                safe_drop_source(voice.source);
            } else {
                index += 1;
            }
        }
    }

    fn clear_voices(&mut self) {
        while let Some(voice) = self.voices.pop() {
            safe_drop_source(voice.source);
        }
    }

    /// Render mixed audio offline into a new interleaved buffer.
    pub fn render_offline(&mut self, max_frames: usize, chunk_frames: usize) -> Result<Vec<f32>> {
        let channels = usize::from(self.channels);
        anyhow::ensure!(
            chunk_frames > 0,
            "offline audio chunk size must be non-zero"
        );
        let max_samples = max_frames
            .checked_mul(channels)
            .context("offline audio output size overflowed")?;
        let chunk_samples = chunk_frames
            .checked_mul(channels)
            .context("offline audio chunk size overflowed")?;
        anyhow::ensure!(
            max_samples <= MAX_OFFLINE_SAMPLES,
            "offline audio output exceeds the {MAX_OFFLINE_SAMPLES}-sample limit"
        );
        anyhow::ensure!(
            chunk_samples <= MAX_OFFLINE_SAMPLES,
            "offline audio chunk exceeds the {MAX_OFFLINE_SAMPLES}-sample limit"
        );
        let mut output = Vec::new();
        output
            .try_reserve_exact(max_samples.min(chunk_samples))
            .context("failed to reserve offline audio output")?;
        let mut scratch = vec![0.0f32; chunk_samples];
        let mut rendered = 0;
        while rendered < max_frames && self.active_voices() > 0 {
            let frames = chunk_frames.min(max_frames - rendered);
            let needed = frames * channels;
            self.process_inner(&mut scratch[..needed], false);
            output
                .try_reserve(needed)
                .context("failed to grow offline audio output")?;
            output.extend_from_slice(&scratch[..needed]);
            rendered += frames;
        }
        Ok(output)
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.clear_voices();
    }
}

/// Bounds for the event-driven browser live-output bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserAudioEngineConfig {
    channels: u16,
    chunk_frames: usize,
    pending_chunks: usize,
}

impl BrowserAudioEngineConfig {
    /// Build checked worklet bounds.
    ///
    /// `chunk_frames` must be a multiple of the browser's 128-frame render
    /// quantum. The supported range is 128..=4096 frames, 1..=8 channels, and
    /// 2..=32 pending chunks.
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
        })
    }

    /// Interleaved output channels.
    pub fn channels(self) -> u16 {
        self.channels
    }

    /// Frames produced per transferred sample chunk.
    pub fn chunk_frames(self) -> usize {
        self.chunk_frames
    }

    /// Maximum sample chunks queued at the AudioWorklet boundary.
    pub fn pending_chunks(self) -> usize {
        self.pending_chunks
    }
}

impl Default for BrowserAudioEngineConfig {
    fn default() -> Self {
        Self {
            channels: 2,
            chunk_frames: 256,
            pending_chunks: 4,
        }
    }
}

/// Live browser audio driven by an AudioWorklet pull protocol.
///
/// Construction is asynchronous because installing a worklet module returns a
/// browser promise. Call [`Self::resume_async`] directly from a click or key
/// handler when autoplay policy leaves the context suspended. Dropping the
/// owning engine disconnects the graph even when control handles remain.
#[must_use = "dropping the engine stops audio output"]
pub struct AudioEngine {
    inner: Rc<EngineInner>,
    handle: AudioEngineHandle,
}

/// A clonable browser live-engine control facade.
///
/// Handles are weak: they cannot keep an output graph alive after the owning
/// [`AudioEngine`] is dropped.
#[derive(Clone)]
pub struct AudioEngineHandle {
    inner: Weak<EngineInner>,
    clock: AudioClock,
    sample_rate: u32,
    channels: u16,
}

struct EngineInner {
    state: RefCell<EngineState>,
}

struct EngineState {
    context: AudioContext,
    node: AudioWorkletNode,
    port: MessagePort,
    mixer: Mixer,
    config: BrowserAudioEngineConfig,
    next_voice: VoiceId,
    output_scratch: Vec<f32>,
    events: VecDeque<BrowserAudioEvent>,
    last_error: Option<String>,
    last_rendered_frames: u64,
    underrun_frames: u64,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_processor_error: Option<Closure<dyn FnMut(Event)>>,
    closed: bool,
}

impl AudioEngine {
    /// Maximum number of live voices reserved by one engine.
    pub const MAX_VOICES: usize = MAX_ACTIVE_VOICES;

    /// Return an explicit error because browser worklet setup is asynchronous.
    pub fn new() -> Result<Self> {
        anyhow::bail!(LIVE_ENGINE_ASYNC_MESSAGE)
    }

    /// Return an explicit error because browser worklet setup is asynchronous.
    pub fn from_output_device(_device: &crate::AudioOutputDevice) -> Result<Self> {
        anyhow::bail!(LIVE_ENGINE_ASYNC_MESSAGE)
    }

    /// Construct the default bounded AudioWorklet graph asynchronously.
    pub async fn new_async() -> std::result::Result<Self, BrowserAudioError> {
        Self::new_async_with_config(BrowserAudioEngineConfig::default()).await
    }

    /// Construct an AudioWorklet graph with explicit transfer bounds.
    pub async fn new_async_with_config(
        config: BrowserAudioEngineConfig,
    ) -> std::result::Result<Self, BrowserAudioError> {
        validate_worklet_bounds(config.channels, config.chunk_frames, config.pending_chunks)?;
        let context_options = AudioContextOptions::new();
        context_options.set_latency_hint(&JsValue::from_str("interactive"));
        let chunk_samples = config
            .chunk_frames
            .checked_mul(usize::from(config.channels))
            .ok_or_else(|| BrowserAudioError::new(BrowserAudioErrorKind::InvalidConfiguration))?;
        let context = AudioContext::new_with_context_options(&context_options)
            .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::ApiUnavailable))?;
        let pending_context = PendingAudioContext::new(context);
        install_worklet(pending_context.context()).await?;
        let sample_rate = checked_context_sample_rate(pending_context.context().sample_rate())?;
        let (node, port) = create_output_node(pending_context.context(), config)?;
        let context = pending_context.into_inner();
        let mixer = Mixer::new(sample_rate, config.channels);
        let clock = mixer.clock();
        let inner = Rc::new(EngineInner {
            state: RefCell::new(EngineState {
                context,
                node,
                port,
                mixer,
                config,
                next_voice: 1,
                output_scratch: vec![0.0; chunk_samples],
                events: VecDeque::with_capacity(ENGINE_EVENT_CAPACITY),
                last_error: None,
                last_rendered_frames: 0,
                underrun_frames: 0,
                on_message: None,
                on_processor_error: None,
                closed: false,
            }),
        });
        install_engine_handlers(&inner);
        let handle = AudioEngineHandle {
            inner: Rc::downgrade(&inner),
            clock,
            sample_rate,
            channels: config.channels,
        };
        Ok(Self { inner, handle })
    }

    /// Construct a graph for the default browser output route.
    ///
    /// Stable non-default sink routing is not yet interoperable. Passing a
    /// non-default descriptor returns a typed error instead of silently routing
    /// elsewhere.
    pub async fn from_output_device_async(
        device: &crate::AudioOutputDevice,
    ) -> std::result::Result<Self, BrowserAudioError> {
        if !device.is_default() {
            return Err(BrowserAudioError::new(
                BrowserAudioErrorKind::OutputRoutingUnsupported,
            ));
        }
        Self::new_async().await
    }

    /// Resume rendering. Invoke this from a transient user activation.
    pub async fn resume_async(&self) -> std::result::Result<(), BrowserAudioError> {
        let promise = {
            let state = self.inner.state.borrow();
            if state.closed {
                return Err(BrowserAudioError::new(BrowserAudioErrorKind::Closed));
            }
            state.context.resume().map_err(|_| {
                BrowserAudioError::new(BrowserAudioErrorKind::UserActivationRequired)
            })?
        };
        JsFuture::from(promise).await.map_err(|error| {
            let classified =
                classify_js_error(&error, BrowserAudioErrorKind::UserActivationRequired);
            if classified.kind() == BrowserAudioErrorKind::PermissionDenied {
                BrowserAudioError::new(BrowserAudioErrorKind::UserActivationRequired)
            } else {
                classified
            }
        })?;
        push_engine_event(&self.inner, BrowserAudioEvent::Running);
        Ok(())
    }

    /// Suspend rendering without destroying voices or queued graph state.
    pub async fn suspend_async(&self) -> std::result::Result<(), BrowserAudioError> {
        let promise = {
            let state = self.inner.state.borrow();
            if state.closed {
                return Err(BrowserAudioError::new(BrowserAudioErrorKind::Closed));
            }
            state
                .context
                .suspend()
                .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::Closed))?
        };
        JsFuture::from(promise)
            .await
            .map(|_| ())
            .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::Closed))
    }

    /// Disconnect the worklet and await `AudioContext.close()`.
    pub async fn close_async(self) -> std::result::Result<(), BrowserAudioError> {
        let promise = self.inner.shutdown();
        if let Some(promise) = promise {
            JsFuture::from(promise)
                .await
                .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::Closed))?;
        }
        Ok(())
    }

    /// The shared master clock, updated from worklet-rendered frames.
    pub fn clock(&self) -> AudioClock {
        self.handle.clock()
    }

    /// The actual browser output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.handle.sample_rate()
    }

    /// The output channel count.
    pub fn channels(&self) -> u16 {
        self.handle.channels()
    }

    /// Return a clonable weak control handle.
    pub fn handle(&self) -> AudioEngineHandle {
        self.handle.clone()
    }

    /// Return and clear the most recent asynchronous engine error.
    pub fn take_error(&self) -> Option<String> {
        self.handle.take_error()
    }

    /// Poll a bounded lifecycle or pressure event.
    pub fn poll_event(&self) -> Option<BrowserAudioEvent> {
        self.handle.poll_event()
    }

    /// Cumulative worklet underrun frames.
    pub fn underrun_frames(&self) -> u64 {
        self.handle.underrun_frames()
    }

    /// Return the number of live voices.
    pub fn active_voices(&self) -> usize {
        self.handle.active_voices()
    }

    /// Add a bounded live source.
    pub fn play_source(&self, source: Box<dyn SampleSource>, gain: f32) -> Result<VoiceId> {
        self.handle.play_source(source, gain)
    }

    /// Stop one live source.
    pub fn stop_voice(&self, id: VoiceId) -> Result<()> {
        self.handle.stop_voice(id)
    }

    /// Change one live source gain.
    pub fn set_voice_gain(&self, id: VoiceId, gain: f32) -> Result<()> {
        self.handle.set_voice_gain(id, gain)
    }

    /// Change the master mixer gain.
    pub fn set_master_gain(&self, gain: f32) -> Result<()> {
        self.handle.set_master_gain(gain)
    }
}

impl AudioEngineHandle {
    /// Maximum number of live voices reserved by one engine.
    pub const MAX_VOICES: usize = MAX_ACTIVE_VOICES;

    /// The shared master clock.
    pub fn clock(&self) -> AudioClock {
        self.clock.clone()
    }

    /// The actual browser output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The output channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Return and clear the most recent asynchronous engine error.
    pub fn take_error(&self) -> Option<String> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.state.borrow_mut().last_error.take())
    }

    /// Poll a bounded lifecycle or pressure event.
    pub fn poll_event(&self) -> Option<BrowserAudioEvent> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.state.borrow_mut().events.pop_front())
    }

    /// Cumulative worklet underrun frames.
    pub fn underrun_frames(&self) -> u64 {
        self.inner
            .upgrade()
            .map_or(0, |inner| inner.state.borrow().underrun_frames)
    }

    /// Return the number of live voices.
    pub fn active_voices(&self) -> usize {
        self.inner
            .upgrade()
            .map_or(0, |inner| inner.state.borrow().mixer.active_voices())
    }

    /// Return whether the owning engine has a running browser context.
    pub fn is_running(&self) -> bool {
        self.inner.upgrade().is_some_and(|inner| {
            let state = inner.state.borrow();
            !state.closed && state.context.state() == AudioContextState::Running
        })
    }

    /// Add a live source or return bounded voice backpressure.
    pub fn play_source(&self, source: Box<dyn SampleSource>, gain: f32) -> Result<VoiceId> {
        let Some(inner) = self.inner.upgrade() else {
            safe_drop_source(source);
            anyhow::bail!(BrowserAudioError::new(BrowserAudioErrorKind::Closed));
        };
        let mut state = inner.state.borrow_mut();
        if state.closed {
            safe_drop_source(source);
            anyhow::bail!(BrowserAudioError::new(BrowserAudioErrorKind::Closed));
        }
        if state.mixer.active_voices() >= MAX_ACTIVE_VOICES {
            safe_drop_source(source);
            anyhow::bail!(BrowserAudioError::new(BrowserAudioErrorKind::Backpressure));
        }
        let id = state.next_voice;
        state.next_voice = state
            .next_voice
            .checked_add(1)
            .context("browser voice id exhausted")?;
        state.mixer.insert_voice(id, source, gain)?;
        if post_control(&state.port, "wake").is_err() {
            state.mixer.remove_voice(id);
            anyhow::bail!(BrowserAudioError::new(BrowserAudioErrorKind::Processor));
        }
        Ok(id)
    }

    /// Stop one live source.
    pub fn stop_voice(&self, id: VoiceId) -> Result<()> {
        let inner = self
            .inner
            .upgrade()
            .context("browser audio engine is closed")?;
        let mut state = inner.state.borrow_mut();
        anyhow::ensure!(!state.closed, "browser audio engine is closed");
        anyhow::ensure!(state.mixer.remove_voice(id), "audio voice does not exist");
        Ok(())
    }

    /// Change one live source gain.
    pub fn set_voice_gain(&self, id: VoiceId, gain: f32) -> Result<()> {
        let inner = self
            .inner
            .upgrade()
            .context("browser audio engine is closed")?;
        let mut state = inner.state.borrow_mut();
        anyhow::ensure!(!state.closed, "browser audio engine is closed");
        anyhow::ensure!(
            state.mixer.set_voice_gain(id, gain),
            "audio voice does not exist"
        );
        Ok(())
    }

    /// Change the master mixer gain.
    pub fn set_master_gain(&self, gain: f32) -> Result<()> {
        let inner = self
            .inner
            .upgrade()
            .context("browser audio engine is closed")?;
        let mut state = inner.state.borrow_mut();
        anyhow::ensure!(!state.closed, "browser audio engine is closed");
        state.mixer.set_master_gain(gain);
        Ok(())
    }
}

impl EngineInner {
    fn shutdown(&self) -> Option<js_sys::Promise> {
        let mut state = self.state.borrow_mut();
        if state.closed {
            return None;
        }
        state.closed = true;
        state.port.set_onmessage(None);
        state.node.set_onprocessorerror(None);
        let _ = post_control(&state.port, "stop");
        let _ = state.node.disconnect();
        state.port.close();
        state.on_message.take();
        state.on_processor_error.take();
        state.mixer.clear_voices();
        push_bounded_event(&mut state.events, BrowserAudioEvent::Closed);
        state.context.close().ok()
    }
}

impl Drop for EngineInner {
    fn drop(&mut self) {
        if let Some(promise) = self.shutdown() {
            spawn_local(async move {
                let _ = JsFuture::from(promise).await;
            });
        }
    }
}

fn install_engine_handlers(inner: &Rc<EngineInner>) {
    let weak = Rc::downgrade(inner);
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event| {
        if let Some(inner) = weak.upgrade() {
            handle_output_request(&inner, event);
        }
    });
    let weak = Rc::downgrade(inner);
    let on_processor_error = Closure::<dyn FnMut(Event)>::new(move |_| {
        if let Some(inner) = weak.upgrade() {
            {
                let mut state = inner.state.borrow_mut();
                state.last_error = Some("browser audio worklet processor failed".to_string());
                push_bounded_event(&mut state.events, BrowserAudioEvent::ProcessorError);
            }
            shutdown_engine_detached(inner);
        }
    });
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
}

fn handle_output_request(inner: &Rc<EngineInner>, event: MessageEvent) {
    if message_type(&event.data()).as_deref() != Some("need") {
        return;
    }
    let mut state = inner.state.borrow_mut();
    if state.closed {
        return;
    }
    let request = bounded_output_request(
        number_property(&event.data(), "count"),
        number_property(&event.data(), "rendered"),
        number_property(&event.data(), "underruns"),
        state.config.pending_chunks,
    );
    if let Some(rendered) = request.rendered_frames {
        let newly_rendered = rendered.saturating_sub(state.last_rendered_frames);
        state.last_rendered_frames = rendered;
        state.mixer.clock().advance(newly_rendered);
    }
    if let Some(underruns) = request.underrun_frames {
        if underruns > state.underrun_frames {
            state.underrun_frames = underruns;
            push_bounded_event(
                &mut state.events,
                BrowserAudioEvent::OutputUnderrun {
                    total_frames: underruns,
                },
            );
        }
    }
    if state.mixer.active_voices() == 0 {
        let _ = post_control(&state.port, "idle");
        return;
    }
    let chunks = Array::new();
    let transfers = Array::new();
    for _ in 0..request.chunks {
        let EngineState {
            mixer,
            output_scratch,
            ..
        } = &mut *state;
        mixer.process_inner(output_scratch, false);
        let array = Float32Array::from(output_scratch.as_slice());
        transfers.push(&array.buffer());
        chunks.push(&array);
    }
    let message = Object::new();
    if set_property(&message, "type", &JsValue::from_str("chunks")).is_err()
        || set_property(&message, "chunks", chunks.as_ref()).is_err()
        || state
            .port
            .post_message_with_transferable(message.as_ref(), transfers.as_ref())
            .is_err()
    {
        state.last_error = Some("browser audio worklet queue rejected samples".to_string());
        push_bounded_event(&mut state.events, BrowserAudioEvent::ProcessorError);
        drop(state);
        shutdown_engine_detached(Rc::clone(inner));
    }
}

fn create_output_node(
    context: &AudioContext,
    config: BrowserAudioEngineConfig,
) -> std::result::Result<(AudioWorkletNode, MessagePort), BrowserAudioError> {
    let options = AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(0);
    options.set_number_of_outputs(1);
    options.set_channel_count(u32::from(config.channels));
    let output_channels = Array::new();
    output_channels.push(&JsValue::from(u32::from(config.channels)));
    options.set_output_channel_count(output_channels.as_ref());
    let processor_options = Object::new();
    set_property(
        &processor_options,
        "channels",
        &JsValue::from(u32::from(config.channels)),
    )?;
    set_property(
        &processor_options,
        "chunkFrames",
        &JsValue::from(config.chunk_frames as u32),
    )?;
    set_property(
        &processor_options,
        "queueChunks",
        &JsValue::from(config.pending_chunks as u32),
    )?;
    options.set_processor_options(Some(&processor_options));
    let node = AudioWorkletNode::new_with_options(context, "kael-output-v1", &options)
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))?;
    if node
        .connect_with_audio_node(&context.destination())
        .is_err()
    {
        let _ = node.disconnect();
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::WorkletUnavailable,
        ));
    }
    let port = match node.port() {
        Ok(port) => port,
        Err(_) => {
            let _ = node.disconnect();
            return Err(BrowserAudioError::new(
                BrowserAudioErrorKind::WorkletUnavailable,
            ));
        }
    };
    Ok((node, port))
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

fn set_property(
    object: &Object,
    name: &str,
    value: &JsValue,
) -> std::result::Result<(), BrowserAudioError> {
    Reflect::set(object, &JsValue::from_str(name), value)
        .map(|_| ())
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))
}

fn message_type(value: &JsValue) -> Option<String> {
    Reflect::get(value, &JsValue::from_str("type"))
        .ok()
        .and_then(|value| value.as_string())
}

fn number_property(value: &JsValue, name: &str) -> Option<f64> {
    Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite())
}

fn post_control(
    port: &MessagePort,
    message_type: &str,
) -> std::result::Result<(), BrowserAudioError> {
    let message = Object::new();
    set_property(&message, "type", &JsValue::from_str(message_type))?;
    port.post_message(message.as_ref())
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::Processor))
}

fn push_engine_event(inner: &Rc<EngineInner>, event: BrowserAudioEvent) {
    let mut state = inner.state.borrow_mut();
    push_bounded_event(&mut state.events, event);
}

fn push_bounded_event(events: &mut VecDeque<BrowserAudioEvent>, event: BrowserAudioEvent) {
    if events.len() == ENGINE_EVENT_CAPACITY {
        events.pop_front();
    }
    events.push_back(event);
}

fn shutdown_engine_detached(inner: Rc<EngineInner>) {
    spawn_local(async move {
        if let Some(promise) = inner.shutdown() {
            let _ = JsFuture::from(promise).await;
        }
    });
}

fn finite_gain(gain: f32) -> f32 {
    if gain.is_finite() { gain.max(0.0) } else { 0.0 }
}

/// Linearly resample interleaved `input` from `from_rate` to `to_rate`.
pub fn resample_linear(
    input: &[f32],
    channels: u16,
    from_rate: u32,
    to_rate: u32,
) -> Result<Vec<f32>> {
    anyhow::ensure!(channels > 0, "audio channel count must be non-zero");
    anyhow::ensure!(
        from_rate > 0 && to_rate > 0,
        "audio sample rates must be non-zero"
    );
    let channels = usize::from(channels);
    anyhow::ensure!(
        input.len().is_multiple_of(channels),
        "interleaved audio contains an incomplete frame"
    );
    anyhow::ensure!(
        input.iter().all(|sample| sample.is_finite()),
        "audio samples must be finite"
    );
    anyhow::ensure!(
        input.len() <= MAX_OFFLINE_SAMPLES,
        "resampler input exceeds the {MAX_OFFLINE_SAMPLES}-sample limit"
    );
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let input_frames = input.len() / channels;
    if from_rate == to_rate {
        return Ok(input.to_vec());
    }
    let numerator = (input_frames as u128) * u128::from(to_rate);
    let output_frames = (numerator + u128::from(from_rate / 2)) / u128::from(from_rate);
    let output_frames =
        usize::try_from(output_frames.max(1)).context("resampled frame count is too large")?;
    let output_samples = output_frames
        .checked_mul(channels)
        .context("resampled output size overflowed")?;
    anyhow::ensure!(
        output_samples <= MAX_OFFLINE_SAMPLES,
        "resampled output exceeds the {MAX_OFFLINE_SAMPLES}-sample limit"
    );
    let ratio = f64::from(to_rate) / f64::from(from_rate);
    let mut output = vec![0.0f32; output_samples];
    for output_frame in 0..output_frames {
        let source_position = output_frame as f64 / ratio;
        let lower = (source_position.floor() as usize).min(input_frames - 1);
        let fraction = (source_position - lower as f64) as f32;
        let upper = (lower + 1).min(input_frames - 1);
        for channel in 0..channels {
            let a = input[lower * channels + channel];
            let b = input[upper * channels + channel];
            output[output_frame * channels + channel] = a + (b - a) * fraction;
        }
    }
    Ok(output)
}

fn safe_drop_source(source: Box<dyn SampleSource>) {
    if catch_unwind(AssertUnwindSafe(|| drop(source))).is_err() {
        log::error!("audio source destructor panicked; panic contained by the browser mixer");
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioEngine, BufferSource, Mixer};

    #[test]
    fn offline_mixing_remains_available_in_browser_builds() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer
            .insert_voice(1, Box::new(BufferSource::new(vec![0.25, -0.25], 2)), 1.0)
            .unwrap();
        assert_eq!(mixer.render_offline(1, 1).unwrap(), [0.25, -0.25]);
    }

    #[test]
    fn live_engine_fails_explicitly() {
        assert!(AudioEngine::new().is_err());
    }
}
