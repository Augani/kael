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

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result};
use crossbeam_channel::{Sender, TrySendError, bounded};
use parking_lot::Mutex;

const MAX_OFFLINE_SAMPLES: usize = 64 * 1024 * 1024;
const MAX_MIXER_VOICES: usize = 4 * 1024;
const MAX_ACTIVE_VOICES: usize = 1024;
const MAX_PENDING_COMMANDS: usize = 2 * 1024;
const MAX_PENDING_CONTROLS: usize = 1024;
const REALTIME_MIX_CHUNK_FRAMES: usize = 2 * 1024;
const INITIAL_VOICE_CAPACITY: usize = 64;
pub(crate) const MAX_REALTIME_CHANNELS: u16 = 256;

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
    /// With unwind-enabled builds, a panic is contained and the source is retired.
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
    ended: bool,
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
    retire_sender: Option<Sender<Box<dyn SampleSource>>>,
    voice_slots: Option<Arc<AtomicUsize>>,
    callback_fault: Option<Arc<AtomicBool>>,
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
            retire_sender: None,
            voice_slots: None,
            callback_fault: None,
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
    /// same id. Returns an error when the mixer reaches its bounded voice capacity
    /// or cannot reserve storage.
    pub fn insert_voice(
        &mut self,
        id: VoiceId,
        source: Box<dyn SampleSource>,
        gain: f32,
    ) -> Result<()> {
        self.remove_voice(id);
        let limit = if self.retire_sender.is_some() {
            MAX_ACTIVE_VOICES
        } else {
            MAX_MIXER_VOICES
        };
        if self.voices.len() >= limit {
            safe_drop_source(source);
            anyhow::bail!("audio mixer exceeds the {limit}-voice limit");
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
        if let Some(voice) = self
            .voices
            .iter_mut()
            .find(|voice| voice.id == id && !voice.ended)
        {
            voice.gain = finite_gain(gain);
            true
        } else {
            false
        }
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
        if self.retire_sender.is_some() {
            self.voices[index].ended = true;
        } else {
            let voice = self.voices.swap_remove(index);
            safe_drop_source(voice.source);
        }
        true
    }

    /// Number of currently active voices.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| !voice.ended).count()
    }

    /// Sum all active voices into `out` (interleaved), apply the master gain, and
    /// advance the master clock by the number of frames rendered. Voices that end
    /// are retired; live engines destroy them away from the device callback.
    pub fn process(&mut self, out: &mut [f32]) {
        self.process_inner(out, true);
    }

    fn process_inner(&mut self, out: &mut [f32], advance_clock: bool) {
        self.retire_ended();
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

        for voice in &mut self.voices {
            if voice.ended {
                continue;
            }
            for sample in scratch[..needed].iter_mut() {
                *sample = 0.0;
            }
            let written_frames = match catch_unwind(AssertUnwindSafe(|| {
                voice.source.fill(&mut scratch[..needed], channels_u16)
            })) {
                Ok(written) => written.min(frames),
                Err(_) => {
                    voice.ended = true;
                    if let Some(fault) = &self.callback_fault {
                        fault.store(true, Ordering::Release);
                    }
                    0
                }
            };
            let written = written_frames.saturating_mul(channels).min(needed);
            let gain = voice.gain;
            for (out_sample, src) in out[..written].iter_mut().zip(&scratch[..written]) {
                if src.is_finite() {
                    let sum = f64::from(*out_sample) + f64::from(gain) * f64::from(*src);
                    *out_sample = sum.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
                }
            }
            if written_frames < frames {
                voice.ended = true;
            }
        }

        self.scratch = scratch;

        let master_gain = self.master_gain;
        if (master_gain - 1.0).abs() > f32::EPSILON {
            for sample in out[..needed].iter_mut() {
                let scaled = f64::from(*sample) * f64::from(master_gain);
                *sample = scaled.clamp(f64::from(f32::MIN), f64::from(f32::MAX)) as f32;
            }
        }

        if advance_clock {
            self.clock
                .advance(u64::try_from(frames).unwrap_or(u64::MAX));
        }
        self.retire_ended();
    }

    fn prepare_realtime(
        &mut self,
        samples: usize,
        retire_sender: Sender<Box<dyn SampleSource>>,
        voice_slots: Arc<AtomicUsize>,
        callback_fault: Arc<AtomicBool>,
    ) -> Result<()> {
        if self.voices.capacity() < MAX_ACTIVE_VOICES {
            self.voices
                .try_reserve_exact(MAX_ACTIVE_VOICES - self.voices.len())
                .context("failed to reserve live audio voices")?;
        }
        if self.scratch.len() < samples {
            self.scratch
                .try_reserve_exact(samples - self.scratch.len())
                .context("failed to reserve the audio callback buffer")?;
            self.scratch.resize(samples, 0.0);
        }
        self.retire_sender = Some(retire_sender);
        self.voice_slots = Some(voice_slots);
        self.callback_fault = Some(callback_fault);
        Ok(())
    }

    fn retire_ended(&mut self) {
        let mut index = 0;
        while index < self.voices.len() {
            if !self.voices[index].ended {
                index += 1;
                continue;
            }
            let Voice {
                id,
                source,
                gain,
                ended,
            } = self.voices.swap_remove(index);
            let Some(sender) = self.retire_sender.as_ref() else {
                safe_drop_source(source);
                continue;
            };
            match sender.try_send(source) {
                Ok(()) => {
                    self.release_voice_slot();
                }
                Err(TrySendError::Full(source)) => {
                    self.voices.push(Voice {
                        id,
                        source,
                        gain,
                        ended,
                    });
                    break;
                }
                Err(TrySendError::Disconnected(source)) => {
                    if let Some(fault) = &self.callback_fault {
                        fault.store(true, Ordering::Release);
                    }
                    self.voices.push(Voice {
                        id,
                        source,
                        gain,
                        ended,
                    });
                    break;
                }
            }
        }
    }

    fn release_voice_slot(&self) {
        if let Some(slots) = &self.voice_slots {
            release_voice_reservation(slots);
        }
    }

    fn report_callback_fault(&self) {
        if let Some(fault) = &self.callback_fault {
            fault.store(true, Ordering::Release);
        }
    }

    /// Render mixed audio offline (faster than real time) into a new interleaved
    /// buffer, processing `chunk_frames` at a time and stopping once all voices
    /// have ended or `max_frames` is reached. The primitive for export mixdown.
    /// Offline rendering does not advance the device-facing [`AudioClock`].
    pub fn render_offline(&mut self, max_frames: usize, chunk_frames: usize) -> Result<Vec<f32>> {
        let channels = self.channels as usize;
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
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(chunk_samples)
            .context("failed to reserve offline audio scratch space")?;
        scratch.resize(chunk_samples, 0.0f32);
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
        while let Some(voice) = self.voices.pop() {
            self.release_voice_slot();
            safe_drop_source(voice.source);
        }
    }
}

fn finite_gain(gain: f32) -> f32 {
    if gain.is_finite() { gain.max(0.0) } else { 0.0 }
}

/// Linearly resample interleaved `input` from `from_rate` to `to_rate`.
///
/// Linear interpolation is fast and predictable for UI sounds and previews.
/// Use a band-limited resampler when final mastering quality is required. The
/// function rejects incomplete frames, non-finite samples, and oversized output.
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
    let in_frames = input.len() / channels;
    if from_rate == to_rate {
        let mut output = Vec::new();
        output
            .try_reserve_exact(input.len())
            .context("failed to reserve resampled audio output")?;
        output.extend_from_slice(input);
        return Ok(output);
    }
    let numerator = (in_frames as u128) * u128::from(to_rate);
    let out_frames = (numerator + u128::from(from_rate / 2)) / u128::from(from_rate);
    let out_frames =
        usize::try_from(out_frames.max(1)).context("resampled frame count is too large")?;
    let out_samples = out_frames
        .checked_mul(channels)
        .context("resampled output size overflowed")?;
    anyhow::ensure!(
        out_samples <= MAX_OFFLINE_SAMPLES,
        "resampled output exceeds the {MAX_OFFLINE_SAMPLES}-sample limit"
    );
    let ratio = to_rate as f64 / from_rate as f64;
    let mut out = Vec::new();
    out.try_reserve_exact(out_samples)
        .context("failed to reserve resampled audio output")?;
    out.resize(out_samples, 0.0f32);
    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 / ratio;
        let lower = (src_pos.floor() as usize).min(in_frames - 1);
        let frac = (src_pos - lower as f64) as f32;
        let upper = (lower + 1).min(in_frames - 1);
        for channel in 0..channels {
            let a = input[lower * channels + channel];
            let b = input[upper * channels + channel];
            out[out_frame * channels + channel] = a + (b - a) * frac;
        }
    }
    Ok(out)
}

fn safe_drop_source(source: Box<dyn SampleSource>) {
    if catch_unwind(AssertUnwindSafe(|| drop(source))).is_err() {
        log::error!(
            "audio source destructor panicked; panic contained outside the device callback"
        );
    }
}

fn release_voice_reservation(slots: &AtomicUsize) {
    let released = slots.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        current.checked_sub(1)
    });
    debug_assert!(released.is_ok(), "audio voice reservation underflowed");
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
        MixerCommand::Add { id, source, gain } => {
            if mixer.insert_voice(id, source, gain).is_err() {
                mixer.release_voice_slot();
                mixer.report_callback_fault();
            }
        }
        MixerCommand::Remove(id) => {
            mixer.remove_voice(id);
        }
        MixerCommand::SetVoiceGain(id, gain) => {
            mixer.set_voice_gain(id, gain);
        }
        MixerCommand::SetMasterGain(gain) => mixer.set_master_gain(gain),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnqueueOutcome {
    Queued,
    CancelledPendingVoice,
}

fn enqueue_command(
    commands: &Mutex<Vec<MixerCommand>>,
    running: &AtomicBool,
    command: MixerCommand,
) -> Result<EnqueueOutcome> {
    let mut pending = commands.lock();
    if !running.load(Ordering::Acquire) {
        drop(pending);
        safe_drop_command(command);
        anyhow::bail!("audio output stream is no longer running");
    }

    match &command {
        MixerCommand::SetMasterGain(_) => {
            if let Some(existing) = pending
                .iter_mut()
                .rev()
                .find(|queued| matches!(queued, MixerCommand::SetMasterGain(_)))
            {
                *existing = command;
                return Ok(EnqueueOutcome::Queued);
            }
        }
        MixerCommand::SetVoiceGain(id, _) => {
            if pending
                .iter()
                .any(|queued| matches!(queued, MixerCommand::Remove(queued_id) if queued_id == id))
            {
                return Ok(EnqueueOutcome::Queued);
            }
            if let Some(existing) = pending.iter_mut().rev().find(|queued| {
                matches!(queued, MixerCommand::SetVoiceGain(queued_id, _) if queued_id == id)
            }) {
                *existing = command;
                return Ok(EnqueueOutcome::Queued);
            }
        }
        MixerCommand::Remove(id) => {
            if let Some(position) = pending.iter().position(
                |queued| matches!(queued, MixerCommand::Add { id: queued_id, .. } if queued_id == id),
            ) {
                let removed = pending.remove(position);
                pending.retain(
                    |queued| !matches!(queued, MixerCommand::SetVoiceGain(queued_id, _) if queued_id == id),
                );
                drop(pending);
                safe_drop_command(removed);
                return Ok(EnqueueOutcome::CancelledPendingVoice);
            }
            pending.retain(
                |queued| !matches!(queued, MixerCommand::SetVoiceGain(queued_id, _) if queued_id == id),
            );
            if pending
                .iter()
                .any(|queued| matches!(queued, MixerCommand::Remove(queued_id) if queued_id == id))
            {
                return Ok(EnqueueOutcome::Queued);
            }
        }
        MixerCommand::Add { .. } => {}
    }

    let is_control = matches!(
        &command,
        MixerCommand::SetVoiceGain(_, _) | MixerCommand::SetMasterGain(_)
    );
    if is_control
        && pending
            .iter()
            .filter(|queued| {
                matches!(
                    queued,
                    MixerCommand::SetVoiceGain(_, _) | MixerCommand::SetMasterGain(_)
                )
            })
            .count()
            >= MAX_PENDING_CONTROLS
    {
        drop(pending);
        safe_drop_command(command);
        anyhow::bail!("audio control queue is full ({MAX_PENDING_CONTROLS} pending controls)");
    }

    if pending.len() >= MAX_PENDING_COMMANDS {
        if !is_control {
            if let Some(position) = pending.iter().position(|queued| {
                matches!(
                    queued,
                    MixerCommand::SetVoiceGain(_, _) | MixerCommand::SetMasterGain(_)
                )
            }) {
                pending.remove(position);
            }
        }
        if pending.len() >= MAX_PENDING_COMMANDS {
            drop(pending);
            safe_drop_command(command);
            anyhow::bail!("audio command queue is full ({MAX_PENDING_COMMANDS} pending commands)");
        }
    }
    if let Err(error) = pending.try_reserve(1) {
        drop(pending);
        safe_drop_command(command);
        return Err(error).context("failed to reserve audio command queue capacity");
    }
    pending.push(command);
    Ok(EnqueueOutcome::Queued)
}

fn safe_drop_command(command: MixerCommand) {
    if let MixerCommand::Add { source, .. } = command {
        safe_drop_source(source);
    }
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut mixer: Mixer,
    commands: Arc<Mutex<Vec<MixerCommand>>>,
    last_error: Arc<Mutex<Option<String>>>,
    chunk_samples: usize,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait as _;

    let mut mixed = Vec::new();
    mixed
        .try_reserve_exact(chunk_samples)
        .context("failed to reserve the output conversion buffer")?;
    mixed.resize(chunk_samples, 0.0f32);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _info| {
                if let Some(mut pending) = commands.try_lock() {
                    for command in pending.drain(..) {
                        apply_command(&mut mixer, command);
                    }
                }
                for output in data.chunks_mut(chunk_samples) {
                    let converted = &mut mixed[..output.len()];
                    mixer.process(converted);
                    for (output, sample) in output.iter_mut().zip(converted.iter().copied()) {
                        *output = T::from_sample(sample.clamp(-1.0, 1.0));
                    }
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
/// blocking; the [`AudioClock`] reflects frames submitted by the device callback.
/// All steady-state callback storage is reserved during construction.
#[must_use = "dropping the engine stops audio output"]
pub struct AudioEngine {
    stream: Option<cpal::Stream>,
    handle: AudioEngineHandle,
}

/// A clonable, thread-safe control handle for an [`AudioEngine`].
///
/// Keep the engine itself on the thread that created the host stream. Send this
/// handle to workers that need to enqueue sources, gains, stops, or inspect the
/// device clock and asynchronous errors.
#[derive(Clone)]
pub struct AudioEngineHandle {
    clock: AudioClock,
    commands: Arc<Mutex<Vec<MixerCommand>>>,
    last_error: Arc<Mutex<Option<String>>>,
    callback_fault: Arc<AtomicBool>,
    voice_slots: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
    next_id: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
}

impl AudioEngine {
    /// Maximum number of live and pending voices reserved by one engine.
    pub const MAX_VOICES: usize = MAX_ACTIVE_VOICES;

    /// Open the default output device and start an output stream.
    pub fn new() -> Result<Self> {
        Self::from_output_device(&crate::default_output_device()?)
    }

    /// Open a selected output device and start an output stream.
    pub fn from_output_device(device: &crate::AudioOutputDevice) -> Result<Self> {
        use cpal::traits::{DeviceTrait as _, StreamTrait as _};

        let supported = device
            .device
            .default_output_config()
            .with_context(|| format!("no default output config for {}", device.name()))?;

        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        anyhow::ensure!(sample_rate > 0, "audio output sample rate must be non-zero");
        anyhow::ensure!(
            (1..=MAX_REALTIME_CHANNELS).contains(&channels),
            "unsupported output channel count {channels}; expected 1..={MAX_REALTIME_CHANNELS}"
        );
        let chunk_samples = REALTIME_MIX_CHUNK_FRAMES
            .checked_mul(usize::from(channels))
            .context("audio output callback chunk size overflowed")?;
        let config: cpal::StreamConfig = supported.into();

        let (retire_sender, retire_receiver) = bounded(MAX_ACTIVE_VOICES);
        std::thread::Builder::new()
            .name("kael-audio-cleanup".into())
            .spawn(move || {
                for source in retire_receiver {
                    safe_drop_source(source);
                }
            })
            .context("failed to start audio source cleanup thread")?;

        let voice_slots = Arc::new(AtomicUsize::new(0));
        let callback_fault = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let mut mixer = Mixer::new(sample_rate, channels);
        mixer.prepare_realtime(
            chunk_samples,
            retire_sender,
            voice_slots.clone(),
            callback_fault.clone(),
        )?;
        let clock = mixer.clock();
        let mut pending_commands = Vec::new();
        pending_commands
            .try_reserve_exact(MAX_PENDING_COMMANDS)
            .context("failed to reserve the live audio command queue")?;
        let commands = Arc::new(Mutex::new(pending_commands));
        let last_error = Arc::new(Mutex::new(None));

        let stream = match sample_format {
            cpal::SampleFormat::I8 => build_output_stream::<i8>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::I16 => build_output_stream::<i16>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::I32 => build_output_stream::<i32>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::I64 => build_output_stream::<i64>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::U8 => build_output_stream::<u8>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::U16 => build_output_stream::<u16>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::U32 => build_output_stream::<u32>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::U64 => build_output_stream::<u64>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::F32 => build_output_stream::<f32>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            cpal::SampleFormat::F64 => build_output_stream::<f64>(
                &device.device,
                &config,
                mixer,
                commands.clone(),
                last_error.clone(),
                chunk_samples,
            ),
            _ => anyhow::bail!("unsupported sample format: {sample_format:?}"),
        }?;
        stream.play().context("failed to start output stream")?;

        Ok(Self {
            stream: Some(stream),
            handle: AudioEngineHandle {
                clock,
                commands,
                last_error,
                callback_fault,
                voice_slots,
                running,
                next_id: Arc::new(AtomicU64::new(1)),
                sample_rate,
                channels,
            },
        })
    }

    /// The shared master clock.
    pub fn clock(&self) -> AudioClock {
        self.handle.clock()
    }

    /// The output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.handle.sample_rate()
    }

    /// The output channel count.
    pub fn channels(&self) -> u16 {
        self.handle.channels()
    }

    /// Return a clonable handle for cross-thread control and observation.
    pub fn handle(&self) -> AudioEngineHandle {
        self.handle.clone()
    }

    /// Returns and clears the most recent output-stream or callback error.
    ///
    /// Applications can use this to surface device loss and recreate the
    /// engine after the default output route changes.
    pub fn take_error(&self) -> Option<String> {
        self.handle.take_error()
    }

    /// Return the number of live or pending voices reserved by this engine.
    pub fn active_voices(&self) -> usize {
        self.handle.active_voices()
    }

    /// Start playing `source` at `gain`, returning its voice id.
    pub fn play_source(&self, source: Box<dyn SampleSource>, gain: f32) -> Result<VoiceId> {
        self.handle.play_source(source, gain)
    }

    /// Stop the voice with the given id.
    pub fn stop_voice(&self, id: VoiceId) -> Result<()> {
        self.handle.stop_voice(id)
    }

    /// Set the gain of the voice with the given id.
    pub fn set_voice_gain(&self, id: VoiceId, gain: f32) -> Result<()> {
        self.handle.set_voice_gain(id, gain)
    }

    /// Set the master gain applied to the whole mix.
    pub fn set_master_gain(&self, gain: f32) -> Result<()> {
        self.handle.set_master_gain(gain)
    }
}

impl AudioEngineHandle {
    /// Maximum number of live and pending voices reserved by one engine.
    pub const MAX_VOICES: usize = MAX_ACTIVE_VOICES;

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

    /// Returns and clears the most recent output-stream or callback error.
    pub fn take_error(&self) -> Option<String> {
        self.last_error.lock().take().or_else(|| {
            self.callback_fault.swap(false, Ordering::AcqRel).then(|| {
                "audio callback rejected a faulty source or lost its cleanup service".to_string()
            })
        })
    }

    /// Return the number of live or pending voices reserved by this engine.
    pub fn active_voices(&self) -> usize {
        self.voice_slots.load(Ordering::Acquire)
    }

    /// Return whether the owning engine still has a live host stream.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Start playing `source` at `gain`, returning its voice id.
    pub fn play_source(&self, source: Box<dyn SampleSource>, gain: f32) -> Result<VoiceId> {
        if !self.is_running() {
            safe_drop_source(source);
            anyhow::bail!("audio output stream is no longer running");
        }
        let id = match self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            }) {
            Ok(id) => id,
            Err(_) => {
                safe_drop_source(source);
                anyhow::bail!("audio voice id space exhausted");
            }
        };
        if self
            .voice_slots
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < MAX_ACTIVE_VOICES).then_some(current + 1)
            })
            .is_err()
        {
            safe_drop_source(source);
            anyhow::bail!("audio engine exceeds the {MAX_ACTIVE_VOICES}-voice limit");
        }
        if let Err(error) = enqueue_command(
            &self.commands,
            &self.running,
            MixerCommand::Add { id, source, gain },
        ) {
            release_voice_reservation(&self.voice_slots);
            return Err(error);
        }
        Ok(id)
    }

    /// Stop the voice with the given id.
    pub fn stop_voice(&self, id: VoiceId) -> Result<()> {
        if enqueue_command(&self.commands, &self.running, MixerCommand::Remove(id))?
            == EnqueueOutcome::CancelledPendingVoice
        {
            release_voice_reservation(&self.voice_slots);
        }
        Ok(())
    }

    /// Set the gain of the voice with the given id.
    pub fn set_voice_gain(&self, id: VoiceId, gain: f32) -> Result<()> {
        enqueue_command(
            &self.commands,
            &self.running,
            MixerCommand::SetVoiceGain(id, gain),
        )
        .map(|_| ())
    }

    /// Set the master gain applied to the whole mix.
    pub fn set_master_gain(&self, gain: f32) -> Result<()> {
        enqueue_command(
            &self.commands,
            &self.running,
            MixerCommand::SetMasterGain(gain),
        )
        .map(|_| ())
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        drop(self.stream.take());
        let pending = {
            let mut commands = self.handle.commands.lock();
            self.handle.running.store(false, Ordering::Release);
            std::mem::take(&mut *commands)
        };
        for command in pending {
            let reserved_voice = matches!(&command, MixerCommand::Add { .. });
            safe_drop_command(command);
            if reserved_voice {
                release_voice_reservation(&self.handle.voice_slots);
            }
        }
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

    fn enqueue_test(
        commands: &Mutex<Vec<MixerCommand>>,
        command: MixerCommand,
    ) -> Result<EnqueueOutcome> {
        static RUNNING: AtomicBool = AtomicBool::new(true);
        enqueue_command(commands, &RUNNING, command)
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
    fn control_handles_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<AudioClock>();
        assert_send_sync::<AudioEngineHandle>();
    }

    #[test]
    fn stopped_control_handles_reject_and_release_new_sources() {
        struct DropFlag(Arc<AtomicBool>);
        impl SampleSource for DropFlag {
            fn fill(&mut self, _out: &mut [f32], _channels: u16) -> usize {
                0
            }
        }
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let handle = AudioEngineHandle {
            clock: AudioClock::new(48_000),
            commands: Arc::new(Mutex::new(Vec::new())),
            last_error: Arc::new(Mutex::new(None)),
            callback_fault: Arc::new(AtomicBool::new(false)),
            voice_slots: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            next_id: Arc::new(AtomicU64::new(1)),
            sample_rate: 48_000,
            channels: 2,
        };

        assert!(
            handle
                .play_source(Box::new(DropFlag(dropped.clone())), 1.0)
                .is_err()
        );
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(handle.active_voices(), 0);
        assert!(!handle.is_running());
    }

    #[test]
    fn dropping_engine_releases_pending_voice_reservations() {
        struct DropFlag(Arc<AtomicBool>);
        impl SampleSource for DropFlag {
            fn fill(&mut self, _out: &mut [f32], _channels: u16) -> usize {
                0
            }
        }
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let handle = AudioEngineHandle {
            clock: AudioClock::new(48_000),
            commands: Arc::new(Mutex::new(Vec::new())),
            last_error: Arc::new(Mutex::new(None)),
            callback_fault: Arc::new(AtomicBool::new(false)),
            voice_slots: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicBool::new(true)),
            next_id: Arc::new(AtomicU64::new(1)),
            sample_rate: 48_000,
            channels: 2,
        };
        let engine = AudioEngine {
            stream: None,
            handle: handle.clone(),
        };

        handle
            .play_source(Box::new(DropFlag(dropped.clone())), 1.0)
            .unwrap();
        assert_eq!(handle.active_voices(), 1);
        drop(engine);

        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(handle.active_voices(), 0);
        assert!(!handle.is_running());
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
        mixer
            .insert_voice(1, constant_source(0.5, 64, 2), 1.0)
            .unwrap();
        mixer
            .insert_voice(2, constant_source(0.25, 64, 2), 1.0)
            .unwrap();

        let mut out = vec![0.0f32; 64 * 2];
        mixer.process(&mut out);
        assert!(out.iter().all(|&s| (s - 0.75).abs() < 1e-6));

        mixer.set_master_gain(0.5);
        mixer
            .insert_voice(1, constant_source(0.5, 64, 2), 1.0)
            .unwrap();
        mixer
            .insert_voice(2, constant_source(0.25, 64, 2), 1.0)
            .unwrap();
        let mut out = vec![0.0f32; 64 * 2];
        mixer.process(&mut out);
        assert!(out.iter().all(|&s| (s - 0.375).abs() < 1e-6));
    }

    #[test]
    fn voice_is_dropped_when_source_ends() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer
            .insert_voice(1, constant_source(0.5, 16, 2), 1.0)
            .unwrap();
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
        mixer
            .insert_voice(1, constant_source(0.5, 128, 2), 1.0)
            .unwrap();
        mixer
            .insert_voice(2, constant_source(0.25, 128, 2), 1.0)
            .unwrap();

        let output = mixer.render_offline(10_000, 64).unwrap();
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
        let out = resample_linear(&input, 1, 1000, 2000).unwrap();
        assert_eq!(out.len(), 8);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!(out.iter().all(|&s| (-1.0..=1.0).contains(&s)));
    }

    #[test]
    fn resample_is_identity_for_equal_rates() {
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_linear(&input, 2, 48_000, 48_000).unwrap(), input);
        assert!(resample_linear(&[0.1, 0.2, 0.3], 2, 48_000, 48_000).is_err());
        assert_eq!(resample_linear(&[0.25], 1, 48_000, 1).unwrap(), [0.25]);
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
        mixer
            .insert_voice(1, Box::new(BadSource), f32::INFINITY)
            .unwrap();
        let mut out = [1.0; 4];
        mixer.process(&mut out);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn offline_render_and_resample_reject_unbounded_outputs() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer
            .insert_voice(1, Box::new(SineSource::new(440.0, 48_000, 0.5)), 1.0)
            .unwrap();
        assert!(mixer.render_offline(usize::MAX, 64).is_err());
        assert!(resample_linear(&[0.0], 1, 1, u32::MAX).is_err());
    }

    #[test]
    fn command_queue_is_bounded_and_coalesces_controls() {
        let commands = Mutex::new(Vec::new());
        enqueue_test(&commands, MixerCommand::SetMasterGain(0.5)).unwrap();
        enqueue_test(&commands, MixerCommand::SetMasterGain(0.75)).unwrap();
        enqueue_test(&commands, MixerCommand::SetVoiceGain(7, 0.5)).unwrap();
        enqueue_test(&commands, MixerCommand::SetVoiceGain(7, 0.25)).unwrap();
        assert_eq!(commands.lock().len(), 2);

        commands.lock().clear();
        for id in 0..MAX_PENDING_CONTROLS as u64 {
            enqueue_test(&commands, MixerCommand::SetVoiceGain(id, 0.5)).unwrap();
        }
        assert!(enqueue_test(&commands, MixerCommand::SetVoiceGain(u64::MAX, 0.5)).is_err());

        for id in 0..MAX_ACTIVE_VOICES as u64 {
            enqueue_test(
                &commands,
                MixerCommand::Add {
                    id,
                    source: Box::new(SineSource::new(440.0, 48_000, 0.1).with_frames(1)),
                    gain: 1.0,
                },
            )
            .unwrap();
        }
        assert_eq!(commands.lock().len(), MAX_PENDING_COMMANDS);
        enqueue_test(&commands, MixerCommand::Remove(u64::MAX)).unwrap();
        assert_eq!(commands.lock().len(), MAX_PENDING_COMMANDS);
    }

    #[test]
    fn stopping_a_pending_voice_cancels_it_before_the_callback() {
        let commands = Mutex::new(Vec::new());
        enqueue_test(
            &commands,
            MixerCommand::Add {
                id: 7,
                source: Box::new(SineSource::new(440.0, 48_000, 0.1)),
                gain: 1.0,
            },
        )
        .unwrap();
        enqueue_test(&commands, MixerCommand::SetVoiceGain(7, 0.5)).unwrap();
        let outcome = enqueue_test(&commands, MixerCommand::Remove(7)).unwrap();
        assert_eq!(outcome, EnqueueOutcome::CancelledPendingVoice);
        assert!(commands.lock().is_empty());
    }

    #[test]
    fn realtime_chunks_and_voice_storage_are_bounded() {
        let max_samples = REALTIME_MIX_CHUNK_FRAMES * usize::from(MAX_REALTIME_CHANNELS);
        assert_eq!(max_samples, 512 * 1024);

        let (retire_sender, _retire_receiver) = bounded(MAX_ACTIVE_VOICES);
        let slots = Arc::new(AtomicUsize::new(0));
        let fault = Arc::new(AtomicBool::new(false));
        let mut mixer = Mixer::new(48_000, 2);
        mixer
            .prepare_realtime(max_samples, retire_sender, slots, fault)
            .unwrap();
        assert!(mixer.voices.capacity() >= MAX_ACTIVE_VOICES);
        assert!(mixer.scratch.len() >= max_samples);
    }

    #[test]
    fn ended_sources_are_retired_without_callback_destruction() {
        struct DropFlag(Arc<AtomicBool>);
        impl SampleSource for DropFlag {
            fn fill(&mut self, _out: &mut [f32], _channels: u16) -> usize {
                0
            }
        }
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let (retire_sender, retire_receiver) = bounded(1);
        let slots = Arc::new(AtomicUsize::new(1));
        let fault = Arc::new(AtomicBool::new(false));
        let mut mixer = Mixer::new(48_000, 2);
        mixer
            .prepare_realtime(2, retire_sender, slots.clone(), fault)
            .unwrap();
        mixer
            .insert_voice(1, Box::new(DropFlag(dropped.clone())), 1.0)
            .unwrap();

        mixer.process(&mut [0.0; 2]);

        assert!(!dropped.load(Ordering::Acquire));
        assert_eq!(slots.load(Ordering::Acquire), 0);
        safe_drop_source(retire_receiver.try_recv().unwrap());
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn source_panics_cannot_unwind_through_the_device_callback() {
        struct PanickingSource;
        impl SampleSource for PanickingSource {
            fn fill(&mut self, _out: &mut [f32], _channels: u16) -> usize {
                panic!("source failure");
            }
        }

        let (retire_sender, retire_receiver) = bounded(1);
        let slots = Arc::new(AtomicUsize::new(1));
        let fault = Arc::new(AtomicBool::new(false));
        let mut mixer = Mixer::new(48_000, 2);
        mixer
            .prepare_realtime(2, retire_sender, slots.clone(), fault.clone())
            .unwrap();
        mixer
            .insert_voice(1, Box::new(PanickingSource), 1.0)
            .unwrap();

        let mut output = [1.0; 2];
        mixer.process(&mut output);

        assert_eq!(output, [0.0; 2]);
        assert!(fault.load(Ordering::Acquire));
        assert_eq!(slots.load(Ordering::Acquire), 0);
        safe_drop_source(retire_receiver.try_recv().unwrap());
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
