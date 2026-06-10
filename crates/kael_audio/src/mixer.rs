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
        Duration::from_secs_f64(self.seconds())
    }

    /// Reset the frame counter to zero.
    pub fn reset(&self) {
        self.frames.store(0, Ordering::Release);
    }

    fn advance(&self, frames: u64) {
        self.frames.fetch_add(frames, Ordering::AcqRel);
    }
}

/// A streaming source of interleaved `f32` samples at the mixer's rate.
pub trait SampleSource: Send {
    /// Fill `out` with interleaved samples (`channels` per frame), up to its
    /// capacity. Returns the number of **frames** written; a write shorter than
    /// `out.len() / channels` signals that the source has ended.
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
        if self.cursor + src_channels > self.samples.len() {
            if self.looping && !self.samples.is_empty() {
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
            self.phase += self.phase_increment;
            if self.phase >= std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
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
            voices: Vec::new(),
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
        self.master_gain = gain.max(0.0);
    }

    /// Insert a voice with an explicit id; replaces any existing voice with the
    /// same id.
    pub fn insert_voice(&mut self, id: VoiceId, source: Box<dyn SampleSource>, gain: f32) {
        self.remove_voice(id);
        self.voices.push(Voice {
            id,
            source,
            gain: gain.max(0.0),
        });
    }

    /// Set a voice's gain. Returns `false` if no such voice exists.
    pub fn set_voice_gain(&mut self, id: VoiceId, gain: f32) -> bool {
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == id) {
            voice.gain = gain.max(0.0);
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
            let written_frames = voice.source.fill(&mut scratch[..needed], channels_u16);
            let written = (written_frames * channels).min(needed);
            let gain = voice.gain;
            for (out_sample, src) in out[..written].iter_mut().zip(&scratch[..written]) {
                *out_sample += gain * *src;
            }
            written_frames >= frames
        });

        self.scratch = scratch;

        let master_gain = self.master_gain;
        if (master_gain - 1.0).abs() > f32::EPSILON {
            for sample in out[..needed].iter_mut() {
                *sample *= master_gain;
            }
        }

        self.clock.advance(frames as u64);
    }

    /// Render mixed audio offline (faster than real time) into a new interleaved
    /// buffer, processing `chunk_frames` at a time and stopping once all voices
    /// have ended or `max_frames` is reached. The primitive for export mixdown.
    pub fn render_offline(&mut self, max_frames: usize, chunk_frames: usize) -> Vec<f32> {
        let channels = self.channels as usize;
        let chunk_frames = chunk_frames.max(1);
        let mut output = Vec::with_capacity(max_frames.saturating_mul(channels));
        let mut scratch = vec![0.0f32; chunk_frames * channels];
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

/// Linearly resample interleaved `input` from `from_rate` to `to_rate`.
///
/// This is the interim resampler for the audio spine; the production engine will
/// use a windowed-sinc resampler for higher quality.
pub fn resample_linear(input: &[f32], channels: u16, from_rate: u32, to_rate: u32) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }
    let in_frames = input.len() / channels;
    if in_frames == 0 {
        return Vec::new();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_frames = ((in_frames as f64) * ratio).round() as usize;
    let mut out = vec![0.0f32; out_frames * channels];
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

/// A live audio engine: a [`Mixer`] driven by a real `cpal` output stream.
///
/// Control calls enqueue commands that the audio callback drains without
/// blocking; the [`AudioClock`] reflects frames actually rendered to the device.
pub struct AudioEngine {
    _stream: cpal::Stream,
    clock: AudioClock,
    commands: Arc<Mutex<Vec<MixerCommand>>>,
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
        if sample_format != cpal::SampleFormat::F32 {
            anyhow::bail!("unsupported sample format: {sample_format:?}");
        }

        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let config: cpal::StreamConfig = supported.into();

        let mut mixer = Mixer::new(sample_rate, channels);
        let clock = mixer.clock();
        let commands: Arc<Mutex<Vec<MixerCommand>>> = Arc::new(Mutex::new(Vec::new()));
        let callback_commands = commands.clone();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _info| {
                    if let Some(mut pending) = callback_commands.try_lock() {
                        for command in pending.drain(..) {
                            apply_command(&mut mixer, command);
                        }
                    }
                    mixer.process(data);
                },
                |error| log::error!("audio output stream error: {error}"),
                None,
            )
            .context("failed to build output stream")?;
        stream.play().context("failed to start output stream")?;

        Ok(Self {
            _stream: stream,
            clock,
            commands,
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

    /// Start playing `source` at `gain`, returning its voice id.
    pub fn play_source(&self, source: Box<dyn SampleSource>, gain: f32) -> VoiceId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.commands
            .lock()
            .push(MixerCommand::Add { id, source, gain });
        id
    }

    /// Stop the voice with the given id.
    pub fn stop_voice(&self, id: VoiceId) {
        self.commands.lock().push(MixerCommand::Remove(id));
    }

    /// Set the gain of the voice with the given id.
    pub fn set_voice_gain(&self, id: VoiceId, gain: f32) {
        self.commands
            .lock()
            .push(MixerCommand::SetVoiceGain(id, gain));
    }

    /// Set the master gain applied to the whole mix.
    pub fn set_master_gain(&self, gain: f32) {
        self.commands.lock().push(MixerCommand::SetMasterGain(gain));
    }
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
        engine.play_source(Box::new(SineSource::new(440.0, rate, 0.2)), 0.5);
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            clock.frames() > 0,
            "device clock should advance during playback"
        );
    }
}
