//! Real-time audio mixing graph with a device-sample-counter master clock.
//!
//! This is the spine of the audio engine: a [`Mixer`] sums any number of voices
//! into the output buffer inside the device callback, and an [`AudioClock`]
//! tracks the number of frames actually rendered to the device. The clock is the
//! A/V master — the video present path schedules frames against
//! [`AudioClock::seconds`], so playback position is measured in real device
//! samples rather than wall-clock time (which is what previously caused drift).
//!
//! The [`Mixer`] core is deterministic and device-free, so it is fully unit
//! tested; [`AudioEngine`] wires it to a real `cpal` output stream.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result};
use parking_lot::Mutex;

const MAX_OFFLINE_SAMPLES: usize = 64 * 1024 * 1024;
const MAX_PENDING_COMMANDS: usize = 1024;
const DEFAULT_CALLBACK_FRAMES: usize = 8 * 1024;
const MAX_CALLBACK_FRAMES: usize = 64 * 1024;
const MAX_CALLBACK_SAMPLES: usize = 1024 * 1024;
const INITIAL_VOICE_CAPACITY: usize = 64;

/// Identifier for a voice playing in a [`Mixer`].
pub type VoiceId = u64;

/// Monotonic master clock driven by the audio device's sample counter.
///
/// The audio callback is the sole writer: it advances the clock by the number
/// of frames it renders. Readers (e.g. the video present path) observe how much
/// audio has actually been presented to the device.
#[derive(Clone)]
pub struct AudioClock {
    frames: Arc<AtomicU64>,
    sample_rate: u32,
}

impl AudioClock {
    /// Create a clock for the given output sample rate (per channel, in Hz).
    pub fn new(sample_rate: u32) -> Self {
        Self {
            frames: Arc::new(AtomicU64::new(0)),
            sample_rate: sample_rate.max(1),
        }
    }

    /// The output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Total frames rendered to the device since the last reset.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    /// Playback position in seconds (device frames / sample rate).
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
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
        self.frames.store(0, Ordering::Release);
    }

    fn advance(&self, frames: u64) {
        let _ = self
            .frames
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                Some(current.saturating_add(frames))
            });
    }
}

/// A streaming source of interleaved `f32` samples at the mixer's rate.
pub trait SampleSource: Send {
    /// Fill `out` with interleaved samples (`channels` per frame), up to its
    /// capacity. Returns the number of **frames** written; a write shorter than
    /// `out.len() / channels` signals that the source has ended.
    ///
    /// Live engines call this method on the device thread. Implementations must
    /// not block, perform file or network I/O, or allocate in the steady state.
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
        let src_channels = self.source_channels as usize;
        if self
            .cursor
            .checked_add(src_channels)
            .is_none_or(|end| end > self.samples.len())
        {
            if self.looping && self.samples.len() >= src_channels {
                self.cursor = 0;
            } else {
                return false;
            }
        }
        let src = &self.samples[self.cursor..self.cursor + src_channels];
        for (channel, out) in frame.iter_mut().enumerate().take(channels as usize) {
            *out = if src_channels == 1 {
                src[0]
            } else {
                src[channel.min(src_channels - 1)]
            };
        }
        self.cursor += src_channels;
        true
    }
}

impl SampleSource for BufferSource {
    fn fill(&mut self, out: &mut [f32], channels: u16) -> usize {
        let channels = channels.max(1) as usize;
        let frames = out.len() / channels;
        let mut written = 0;
        for frame_index in 0..frames {
            let start = frame_index * channels;
            if !self.next_frame(channels as u16, &mut out[start..start + channels]) {
                break;
            }
            written += 1;
        }
        written
    }
}

/// A sine-wave generator, primarily for tests and tone playback.
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
    fn fill(&mut self, out: &mut [f32], channels: u16) -> usize {
        let channels = channels.max(1) as usize;
        let frames = out.len() / channels;
        let frames = match self.remaining_frames {
            Some(remaining) => frames.min(remaining),
            None => frames,
        };
        for frame_index in 0..frames {
            let sample = self.amplitude * self.phase.sin();
            self.phase = (self.phase + self.phase_increment).rem_euclid(std::f32::consts::TAU);
            let start = frame_index * channels;
            for out_sample in out[start..start + channels].iter_mut() {
                *out_sample = sample;
            }
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
}

/// A real-time mixing graph: sums active voices into the output buffer and
/// advances the shared [`AudioClock`] by the frames rendered.
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

    /// A clone of the shared master clock.
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

    /// Insert a voice with an explicit id; replaces any existing voice with the
    /// same id.
    pub fn insert_voice(&mut self, id: VoiceId, source: Box<dyn SampleSource>, gain: f32) {
        self.remove_voice(id);
        self.voices.push(Voice {
            id,
            source,
            gain: finite_gain(gain),
        });
    }

    /// Set a voice's gain. Returns `false` if no such voice exists.
    pub fn set_voice_gain(&mut self, id: VoiceId, gain: f32) -> bool {
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == id) {
            voice.gain = finite_gain(gain);
            true
        } else {
            false
        }
    }

    /// Remove a voice. Returns `false` if no such voice exists.
    pub fn remove_voice(&mut self, id: VoiceId) -> bool {
        let before = self.voices.len();
        self.voices.retain(|voice| voice.id != id);
        self.voices.len() != before
    }

    /// Number of currently active voices.
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    /// Sum all active voices into `out` (interleaved), apply the master gain, and
    /// advance the master clock by the number of frames rendered. Voices that end
    /// are dropped.
    pub fn process(&mut self, out: &mut [f32]) {
        let channels = self.channels as usize;
        for sample in out.iter_mut() {
            *sample = 0.0;
        }
        let frames = out.len() / channels;
        if frames == 0 {
            return;
        }
        let needed = frames * channels;

        let mut scratch = std::mem::take(&mut self.scratch);
        if scratch.len() < needed {
            scratch.resize(needed, 0.0);
        }
        let channels_u16 = self.channels;

        self.voices.retain_mut(|voice| {
            for sample in scratch[..needed].iter_mut() {
                *sample = 0.0;
            }
            let written_frames = voice
                .source
                .fill(&mut scratch[..needed], channels_u16)
                .min(frames);
            let written = written_frames.saturating_mul(channels).min(needed);
            let gain = voice.gain;
            for (out_sample, src) in out[..written].iter_mut().zip(&scratch[..written]) {
                if src.is_finite() {
                    let sum = f64::from(*out_sample) + f64::from(gain) * f64::from(*src);
                    *out_sample = sum.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
                }
            }
            written_frames >= frames
        });

        self.scratch = scratch;

        let master_gain = self.master_gain;
        if (master_gain - 1.0).abs() > f32::EPSILON {
            for sample in out[..needed].iter_mut() {
                let scaled = f64::from(*sample) * f64::from(master_gain);
                *sample = scaled.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
            }
        }

        self.clock
            .advance(u64::try_from(frames).unwrap_or(u64::MAX));
    }

    fn prepare_callback_buffer(&mut self, samples: usize) -> Result<()> {
        if self.scratch.len() < samples {
            self.scratch
                .try_reserve_exact(samples - self.scratch.len())
                .context("failed to reserve the audio callback buffer")?;
            self.scratch.resize(samples, 0.0);
        }
        Ok(())
    }

    /// Render mixed audio offline (faster than real time) into a new interleaved
    /// buffer, processing `chunk_frames` at a time and stopping once all voices
    /// have ended or `max_frames` is reached. The primitive for export mixdown.
    pub fn render_offline(&mut self, max_frames: usize, chunk_frames: usize) -> Vec<f32> {
        let channels = self.channels as usize;
        let chunk_frames = chunk_frames.max(1);
        let Some(max_samples) = max_frames.checked_mul(channels) else {
            return Vec::new();
        };
        let Some(chunk_samples) = chunk_frames.checked_mul(channels) else {
            return Vec::new();
        };
        if max_samples > MAX_OFFLINE_SAMPLES || chunk_samples > MAX_OFFLINE_SAMPLES {
            return Vec::new();
        }
        let mut output = Vec::new();
        if output.try_reserve_exact(max_samples).is_err() {
            return Vec::new();
        }
        let mut scratch = Vec::new();
        if scratch.try_reserve_exact(chunk_samples).is_err() {
            return Vec::new();
        }
        scratch.resize(chunk_samples, 0.0f32);
        let mut rendered = 0;
        while rendered < max_frames && self.active_voices() > 0 {
            let frames = chunk_frames.min(max_frames - rendered);
            let needed = frames * channels;
            self.process(&mut scratch[..needed]);
            output.extend_from_slice(&scratch[..needed]);
            rendered += frames;
        }
        output
    }
}

fn finite_gain(gain: f32) -> f32 {
    if gain.is_finite() { gain.max(0.0) } else { 0.0 }
}

/// Linearly resample interleaved `input` from `from_rate` to `to_rate`.
///
/// Linear interpolation is fast and predictable for UI sounds and previews.
/// Use a band-limited resampler when final mastering quality is required.
pub fn resample_linear(input: &[f32], channels: u16, from_rate: u32, to_rate: u32) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    let in_frames = input.len() / channels;
    if in_frames == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input[..in_frames * channels].to_vec();
    }
    let numerator = (in_frames as u128) * u128::from(to_rate);
    let out_frames = (numerator + u128::from(from_rate / 2)) / u128::from(from_rate);
    let Ok(out_frames) = usize::try_from(out_frames) else {
        return Vec::new();
    };
    let Some(out_samples) = out_frames.checked_mul(channels) else {
        return Vec::new();
    };
    if out_samples > MAX_OFFLINE_SAMPLES {
        return Vec::new();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let mut out = Vec::new();
    if out.try_reserve_exact(out_samples).is_err() {
        return Vec::new();
    }
    out.resize(out_samples, 0.0f32);
    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 / ratio;
        let lower = src_pos.floor() as usize;
        let frac = (src_pos - lower as f64) as f32;
        let upper = (lower + 1).min(in_frames - 1);
        for channel in 0..channels {
            let a = input[lower * channels + channel];
            let b = input[upper * channels + channel];
            out[out_frame * channels + channel] = a + (b - a) * frac;
        }
    }
    out
}

enum MixerCommand {
    Add {
        id: VoiceId,
        source: Box<dyn SampleSource>,
        gain: f32,
    },
    Remove(VoiceId),
    SetVoiceGain(VoiceId, f32),
    SetMasterGain(f32),
}

fn apply_command(mixer: &mut Mixer, command: MixerCommand) {
    match command {
        MixerCommand::Add { id, source, gain } => mixer.insert_voice(id, source, gain),
        MixerCommand::Remove(id) => {
            mixer.remove_voice(id);
        }
        MixerCommand::SetVoiceGain(id, gain) => {
            mixer.set_voice_gain(id, gain);
        }
        MixerCommand::SetMasterGain(gain) => mixer.set_master_gain(gain),
    }
}

fn enqueue_command(commands: &Mutex<Vec<MixerCommand>>, command: MixerCommand) -> Result<()> {
    let mut pending = commands.lock();

    match &command {
        MixerCommand::SetMasterGain(_) => {
            if let Some(existing) = pending
                .iter_mut()
                .rev()
                .find(|queued| matches!(queued, MixerCommand::SetMasterGain(_)))
            {
                *existing = command;
                return Ok(());
            }
        }
        MixerCommand::SetVoiceGain(id, _) => {
            if let Some(existing) = pending.iter_mut().rev().find(|queued| {
                matches!(queued, MixerCommand::SetVoiceGain(queued_id, _) if queued_id == id)
            }) {
                *existing = command;
                return Ok(());
            }
        }
        MixerCommand::Remove(id) => {
            if let Some(position) = pending.iter().position(
                |queued| matches!(queued, MixerCommand::Add { id: queued_id, .. } if queued_id == id),
            ) {
                pending.remove(position);
                pending.retain(
                    |queued| !matches!(queued, MixerCommand::SetVoiceGain(queued_id, _) if queued_id == id),
                );
                return Ok(());
            }
            pending.retain(
                |queued| !matches!(queued, MixerCommand::SetVoiceGain(queued_id, _) if queued_id == id),
            );
        }
        MixerCommand::Add { .. } => {}
    }

    if pending.len() >= MAX_PENDING_COMMANDS {
        anyhow::bail!("audio command queue is full ({MAX_PENDING_COMMANDS} pending commands)");
    }
    pending
        .try_reserve(1)
        .context("failed to reserve audio command queue capacity")?;
    pending.push(command);
    Ok(())
}

fn callback_sample_capacity(buffer_size: &cpal::SupportedBufferSize, channels: u16) -> usize {
    let frames = match buffer_size {
        cpal::SupportedBufferSize::Range { max, .. } => {
            usize::try_from(*max).unwrap_or(MAX_CALLBACK_FRAMES)
        }
        cpal::SupportedBufferSize::Unknown => DEFAULT_CALLBACK_FRAMES,
    }
    .clamp(1, MAX_CALLBACK_FRAMES);
    frames
        .checked_mul(usize::from(channels.max(1)))
        .unwrap_or(MAX_CALLBACK_SAMPLES)
        .min(MAX_CALLBACK_SAMPLES)
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut mixer: Mixer,
    commands: Arc<Mutex<Vec<MixerCommand>>>,
    last_error: Arc<Mutex<Option<String>>>,
    callback_samples: usize,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait as _;

    mixer.prepare_callback_buffer(callback_samples)?;
    let mut mixed = Vec::new();
    mixed
        .try_reserve_exact(callback_samples)
        .context("failed to reserve the output conversion buffer")?;
    mixed.resize(callback_samples, 0.0f32);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _info| {
                if data.len() > mixed.len() {
                    for sample in data.iter_mut() {
                        *sample = T::from_sample(0.0);
                    }
                    let frames = data.len() / usize::from(mixer.channels);
                    mixer
                        .clock
                        .advance(u64::try_from(frames).unwrap_or(u64::MAX));
                    return;
                }
                if let Some(mut pending) = commands.try_lock() {
                    for command in pending.drain(..) {
                        apply_command(&mut mixer, command);
                    }
                }
                mixer.process(&mut mixed[..data.len()]);
                for (output, sample) in data.iter_mut().zip(&mixed) {
                    *output = T::from_sample(*sample);
                }
            },
            move |error| {
                let message = bounded_error_message(error.to_string());
                log::error!("audio output stream error: {message}");
                *last_error.lock() = Some(message);
            },
            None,
        )
        .context("failed to build output stream")
}

/// A live audio engine: a [`Mixer`] driven by a real `cpal` output stream.
///
/// Control calls enqueue commands that the audio callback drains without
/// blocking; the [`AudioClock`] reflects frames actually rendered to the device.
pub struct AudioEngine {
    _stream: cpal::Stream,
    clock: AudioClock,
    commands: Arc<Mutex<Vec<MixerCommand>>>,
    last_error: Arc<Mutex<Option<String>>>,
    next_id: AtomicU64,
    sample_rate: u32,
    channels: u16,
}

impl AudioEngine {
    /// Open the default output device and start an output stream.
    pub fn new() -> Result<Self> {
        use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default audio output device")?;
        let supported = device
            .default_output_config()
            .context("no default output config")?;

        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let callback_samples = callback_sample_capacity(supported.buffer_size(), channels);
        let config: cpal::StreamConfig = supported.into();

        let mixer = Mixer::new(sample_rate, channels);
        let clock = mixer.clock();
        let commands: Arc<Mutex<Vec<MixerCommand>>> =
            Arc::new(Mutex::new(Vec::with_capacity(INITIAL_VOICE_CAPACITY)));
        let last_error = Arc::new(Mutex::new(None));

        let stream = match sample_format {
            cpal::SampleFormat::I8 => build_output_stream::<i8>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::I16 => build_output_stream::<i16>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::I32 => build_output_stream::<i32>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::I64 => build_output_stream::<i64>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::U8 => build_output_stream::<u8>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::U16 => build_output_stream::<u16>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::U32 => build_output_stream::<u32>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::U64 => build_output_stream::<u64>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::F32 => build_output_stream::<f32>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            cpal::SampleFormat::F64 => build_output_stream::<f64>(
                &device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                callback_samples,
            ),
            _ => anyhow::bail!("unsupported sample format: {sample_format:?}"),
        }?;
        stream.play().context("failed to start output stream")?;

        Ok(Self {
            _stream: stream,
            clock,
            commands,
            last_error,
            next_id: AtomicU64::new(1),
            sample_rate,
            channels,
        })
    }

    /// The shared master clock.
    pub fn clock(&self) -> AudioClock {
        self.clock.clone()
    }

    /// The output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The output channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns and clears the most recent output-stream error.
    ///
    /// Applications can use this to surface device loss and recreate the
    /// engine after the default output route changes.
    pub fn take_error(&self) -> Option<String> {
        self.last_error.lock().take()
    }

    /// Start playing `source` at `gain`, returning its voice id.
    pub fn play_source(&self, source: Box<dyn SampleSource>, gain: f32) -> Result<VoiceId> {
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("audio voice id space exhausted"))?;
        enqueue_command(&self.commands, MixerCommand::Add { id, source, gain })?;
        Ok(id)
    }

    /// Stop the voice with the given id.
    pub fn stop_voice(&self, id: VoiceId) -> Result<()> {
        enqueue_command(&self.commands, MixerCommand::Remove(id))
    }

    /// Set the gain of the voice with the given id.
    pub fn set_voice_gain(&self, id: VoiceId, gain: f32) -> Result<()> {
        enqueue_command(&self.commands, MixerCommand::SetVoiceGain(id, gain))
    }

    /// Set the master gain applied to the whole mix.
    pub fn set_master_gain(&self, gain: f32) -> Result<()> {
        enqueue_command(&self.commands, MixerCommand::SetMasterGain(gain))
    }
}

fn bounded_error_message(mut message: String) -> String {
    const MAX_ERROR_BYTES: usize = 4 * 1024;
    if message.len() <= MAX_ERROR_BYTES {
        return message;
    }
    let mut boundary = MAX_ERROR_BYTES;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message.truncate(boundary);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant_source(value: f32, frames: usize, channels: u16) -> Box<dyn SampleSource> {
        Box::new(BufferSource::new(
            vec![value; frames * channels as usize],
            channels,
        ))
    }

    #[test]
    fn clock_advances_by_frames_processed() {
        let mut mixer = Mixer::new(48_000, 2);
        let clock = mixer.clock();
        let mut out = vec![0.0f32; 256 * 2];
        mixer.process(&mut out);
        assert_eq!(clock.frames(), 256);
        mixer.process(&mut out);
        assert_eq!(clock.frames(), 512);
        assert!((clock.seconds() - 512.0 / 48_000.0).abs() < 1e-9);
    }

    #[test]
    fn clock_saturates_at_the_full_frame_domain() {
        let clock = AudioClock::new(48_000);
        clock.frames.store(u64::MAX - 1, Ordering::Release);
        clock.advance(10);
        assert_eq!(clock.frames(), u64::MAX);
        assert!(clock.duration() > Duration::ZERO);

        let slow_clock = AudioClock::new(1);
        slow_clock.frames.store(u64::MAX, Ordering::Release);
        assert_eq!(slow_clock.duration(), Duration::from_secs(u64::MAX));
    }

    #[test]
    fn mixer_sums_voices_and_applies_gains() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer.insert_voice(1, constant_source(0.5, 64, 2), 1.0);
        mixer.insert_voice(2, constant_source(0.25, 64, 2), 1.0);

        let mut out = vec![0.0f32; 64 * 2];
        mixer.process(&mut out);
        assert!(out.iter().all(|&s| (s - 0.75).abs() < 1e-6));

        mixer.set_master_gain(0.5);
        mixer.insert_voice(1, constant_source(0.5, 64, 2), 1.0);
        mixer.insert_voice(2, constant_source(0.25, 64, 2), 1.0);
        let mut out = vec![0.0f32; 64 * 2];
        mixer.process(&mut out);
        assert!(out.iter().all(|&s| (s - 0.375).abs() < 1e-6));
    }

    #[test]
    fn voice_is_dropped_when_source_ends() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer.insert_voice(1, constant_source(0.5, 16, 2), 1.0);
        assert_eq!(mixer.active_voices(), 1);

        let mut out = vec![0.0f32; 64 * 2];
        mixer.process(&mut out);
        assert_eq!(mixer.active_voices(), 0);

        assert!((out[0] - 0.5).abs() < 1e-6);
        assert_eq!(out[63 * 2], 0.0);
    }

    #[test]
    fn offline_render_mixes_until_voices_end() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer.insert_voice(1, constant_source(0.5, 128, 2), 1.0);
        mixer.insert_voice(2, constant_source(0.25, 128, 2), 1.0);

        let output = mixer.render_offline(10_000, 64);
        assert!(output.len() >= 128 * 2);
        assert!(
            output[..128 * 2].iter().all(|&s| (s - 0.75).abs() < 1e-6),
            "first 128 frames should be the 0.75 mix"
        );
        assert_eq!(mixer.active_voices(), 0);
    }

    #[test]
    fn buffer_source_upmixes_mono_to_stereo() {
        let mut source = BufferSource::new(vec![0.5, -0.5], 1);
        let mut out = vec![0.0f32; 2 * 2];
        let frames = source.fill(&mut out, 2);
        assert_eq!(frames, 2);
        assert_eq!(out, vec![0.5, 0.5, -0.5, -0.5]);
    }

    #[test]
    fn looping_buffer_rejects_an_incomplete_source_frame() {
        let mut source = BufferSource::new(vec![0.5], 2).looping(true);
        let mut out = [0.0; 2];
        assert_eq!(source.fill(&mut out, 2), 0);
        assert_eq!(out, [0.0; 2]);
    }

    #[test]
    fn sine_source_is_bounded_and_varies() {
        let mut source = SineSource::new(440.0, 48_000, 0.8);
        let mut out = vec![0.0f32; 128];
        let frames = source.fill(&mut out, 1);
        assert_eq!(frames, 128);
        assert!(out.iter().all(|&s| s.abs() <= 0.8 + 1e-6));
        assert!(out.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-4));
    }

    #[test]
    fn sine_source_respects_frame_limit() {
        let mut source = SineSource::new(440.0, 48_000, 0.5).with_frames(10);
        let mut out = vec![0.0f32; 64];
        assert_eq!(source.fill(&mut out, 1), 10);
        assert_eq!(source.fill(&mut out, 1), 0);
    }

    #[test]
    fn resample_doubles_length_when_rate_doubles() {
        let input = vec![0.0, 1.0, 0.0, -1.0];
        let out = resample_linear(&input, 1, 1000, 2000);
        assert_eq!(out.len(), 8);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!(out.iter().all(|&s| (-1.0..=1.0).contains(&s)));
    }

    #[test]
    fn resample_is_identity_for_equal_rates() {
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_linear(&input, 2, 48_000, 48_000), input);
        assert_eq!(
            resample_linear(&[0.1, 0.2, 0.3], 2, 48_000, 48_000),
            vec![0.1, 0.2]
        );
    }

    #[test]
    fn mixer_sanitizes_non_finite_sources_and_gains() {
        struct BadSource;
        impl SampleSource for BadSource {
            fn fill(&mut self, out: &mut [f32], _channels: u16) -> usize {
                out.fill(f32::NAN);
                usize::MAX
            }
        }

        let mut mixer = Mixer::new(48_000, 2);
        mixer.set_master_gain(f32::NAN);
        assert_eq!(mixer.master_gain(), 0.0);
        mixer.insert_voice(1, Box::new(BadSource), f32::INFINITY);
        let mut out = [1.0; 4];
        mixer.process(&mut out);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn offline_render_and_resample_reject_unbounded_outputs() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer.insert_voice(1, Box::new(SineSource::new(440.0, 48_000, 0.5)), 1.0);
        assert!(mixer.render_offline(usize::MAX, 64).is_empty());
        assert!(resample_linear(&[0.0], 1, 1, u32::MAX).is_empty());
    }

    #[test]
    fn command_queue_is_bounded_and_coalesces_controls() {
        let commands = Mutex::new(Vec::new());
        enqueue_command(&commands, MixerCommand::SetMasterGain(0.5)).unwrap();
        enqueue_command(&commands, MixerCommand::SetMasterGain(0.75)).unwrap();
        enqueue_command(&commands, MixerCommand::SetVoiceGain(7, 0.5)).unwrap();
        enqueue_command(&commands, MixerCommand::SetVoiceGain(7, 0.25)).unwrap();
        assert_eq!(commands.lock().len(), 2);

        commands.lock().clear();
        for id in 0..MAX_PENDING_COMMANDS as u64 {
            enqueue_command(
                &commands,
                MixerCommand::Add {
                    id,
                    source: Box::new(SineSource::new(440.0, 48_000, 0.1).with_frames(1)),
                    gain: 1.0,
                },
            )
            .unwrap();
        }
        assert!(
            enqueue_command(
                &commands,
                MixerCommand::Add {
                    id: u64::MAX,
                    source: Box::new(SineSource::new(440.0, 48_000, 0.1).with_frames(1)),
                    gain: 1.0,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn stopping_a_pending_voice_cancels_it_before_the_callback() {
        let commands = Mutex::new(Vec::new());
        enqueue_command(
            &commands,
            MixerCommand::Add {
                id: 7,
                source: Box::new(SineSource::new(440.0, 48_000, 0.1)),
                gain: 1.0,
            },
        )
        .unwrap();
        enqueue_command(&commands, MixerCommand::SetVoiceGain(7, 0.5)).unwrap();
        enqueue_command(&commands, MixerCommand::Remove(7)).unwrap();
        assert!(commands.lock().is_empty());
    }

    #[test]
    fn callback_capacity_is_bounded() {
        assert_eq!(
            callback_sample_capacity(&cpal::SupportedBufferSize::Unknown, 2),
            DEFAULT_CALLBACK_FRAMES * 2
        );
        assert_eq!(
            callback_sample_capacity(
                &cpal::SupportedBufferSize::Range {
                    min: 1,
                    max: u32::MAX,
                },
                u16::MAX,
            ),
            MAX_CALLBACK_SAMPLES
        );
    }

    #[test]
    fn stream_errors_are_utf8_bounded() {
        let message = bounded_error_message("é".repeat(4096));
        assert!(message.len() <= 4 * 1024);
        assert!(message.is_char_boundary(message.len()));
    }

    #[test]
    #[ignore = "opens a real audio device; run manually with --ignored"]
    fn audio_engine_advances_clock_on_real_device() {
        let engine = match AudioEngine::new() {
            Ok(engine) => engine,
            Err(_) => return,
        };
        let clock = engine.clock();
        let rate = engine.sample_rate();
        let _ = engine.play_source(Box::new(SineSource::new(440.0, rate, 0.2)), 0.5);
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            clock.frames() > 0,
            "device clock should advance during playback"
        );
    }
}
