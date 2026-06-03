//! Timeline audio mixdown — the audio counterpart to
//! [`composite_frame`](crate::compositor::composite_frame).
//!
//! [`mix_range`] resolves which source frame each audio track plays at every timeline
//! frame and sums the supplied samples into one buffer. Samples come from an
//! [`AudioProvider`] — the decoder in production, a synthetic generator in tests — so the
//! timeline→audio-buffer path runs deterministically without a decoder.

use std::ops::Range;

use crate::media::{Timeline, TrackType};

/// Supplies decoded mono samples for an audio clip's source frame.
pub trait AudioProvider {
    /// Return `samples_per_frame` mono samples for `source` at `source_frame` — the
    /// audio covering one timeline frame. `None` if unavailable.
    fn samples(
        &self,
        source: &str,
        source_frame: u64,
        samples_per_frame: usize,
    ) -> Option<Vec<f32>>;
}

/// Mix the audio tracks of `timeline` across the frame `range` into a single mono buffer
/// at `sample_rate`, summing overlapping clips at unity gain.
///
/// Returns `None` if the frame rate is invalid or `sample_rate` is not an integer
/// multiple of it (so each timeline frame maps to a whole number of samples). The output
/// length is `range.len() * samples_per_frame`.
pub fn mix_range(
    timeline: &Timeline,
    range: Range<u64>,
    sample_rate: u32,
    provider: &dyn AudioProvider,
) -> Option<Vec<f32>> {
    let fps = timeline.frame_rate;
    if fps <= 0.0 {
        return None;
    }
    let samples_per_frame = sample_rate as f64 / fps;
    if samples_per_frame <= 0.0 || samples_per_frame.fract().abs() > 1e-9 {
        return None;
    }
    let spf = samples_per_frame as usize;
    let frame_count = range.end.saturating_sub(range.start) as usize;
    let mut output = vec![0.0f32; frame_count * spf];

    for (index, frame) in range.enumerate() {
        let base = index * spf;
        for track in &timeline.tracks {
            if track.track_type != TrackType::Audio || !track.enabled {
                continue;
            }
            let Some((clip, source_frame)) = track
                .clips
                .iter()
                .find_map(|clip| clip.source_frame_at(frame).map(|sf| (clip, sf)))
            else {
                continue;
            };
            if let Some(samples) = provider.samples(&clip.source, source_frame, spf) {
                for (offset, sample) in samples.iter().take(spf).enumerate() {
                    output[base + offset] += sample;
                }
            }
        }
    }
    Some(output)
}

/// An [`AudioProvider`] backed by decoded WAV sources, keyed by clip source path. Each
/// source is decoded to mono once; a request returns the per-frame sample window (zero-
/// padded past the end). The concrete uncompressed-audio provider for the mixdown.
#[derive(Debug, Default)]
pub struct WavAudioProvider {
    sources: std::collections::HashMap<String, Vec<f32>>,
}

impl WavAudioProvider {
    /// An empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode `wav_bytes` (PCM-16) and register it under `source` (downmixed to mono).
    /// Returns `false` if the bytes are not a decodable WAV.
    pub fn add_wav(&mut self, source: impl Into<String>, wav_bytes: &[u8]) -> bool {
        let Some((samples, _rate, channels)) = crate::export::decode_wav_pcm16(wav_bytes) else {
            return false;
        };
        self.sources
            .insert(source.into(), downmix_to_mono(samples, channels));
        true
    }
}

fn downmix_to_mono(samples: Vec<f32>, channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples;
    }
    let channels = channels as usize;
    samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

impl AudioProvider for WavAudioProvider {
    fn samples(
        &self,
        source: &str,
        source_frame: u64,
        samples_per_frame: usize,
    ) -> Option<Vec<f32>> {
        let mono = self.sources.get(source)?;
        let start = source_frame as usize * samples_per_frame;
        let mut window = vec![0.0f32; samples_per_frame];
        for (offset, slot) in window.iter_mut().enumerate() {
            if let Some(&sample) = mono.get(start + offset) {
                *slot = sample;
            }
        }
        Some(window)
    }
}

/// How a stereo pan position maps to left/right channel gains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanLaw {
    /// Linear (constant-gain): -6 dB at center.
    Linear,
    /// Constant-power: -3 dB at center (sin/cos law).
    ConstantPower,
    /// Compromise: -4.5 dB at center (geometric mean of the two).
    Minus4_5dB,
}

/// Left/right gains for `pan` in `-1..=1` (-1 hard left, 0 center, +1 hard right) under
/// the given [`PanLaw`]. Pan is clamped to the valid range.
pub fn pan_gains(pan: f32, law: PanLaw) -> (f32, f32) {
    let position = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0; // 0 = left, 1 = right
    let (linear_left, linear_right) = (1.0 - position, position);
    let angle = position * std::f32::consts::FRAC_PI_2;
    let (power_left, power_right) = (angle.cos(), angle.sin());
    match law {
        PanLaw::Linear => (linear_left, linear_right),
        PanLaw::ConstantPower => (power_left, power_right),
        PanLaw::Minus4_5dB => (
            (linear_left * power_left).sqrt(),
            (linear_right * power_right).sqrt(),
        ),
    }
}

/// The floor used for near-silent levels, in dBFS.
pub const SILENCE_DBFS: f32 = -120.0;

/// Convert a linear amplitude to dBFS (`0 dB` = `1.0`). Magnitudes at or below `1e-6`
/// clamp to [`SILENCE_DBFS`].
pub fn linear_to_dbfs(linear: f32) -> f32 {
    let magnitude = linear.abs();
    if magnitude <= 1e-6 {
        SILENCE_DBFS
    } else {
        20.0 * magnitude.log10()
    }
}

/// Convert dBFS to a linear amplitude.
pub fn dbfs_to_linear(dbfs: f32) -> f32 {
    10f32.powf(dbfs / 20.0)
}

/// Peak and RMS level of a sample buffer, in dBFS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelMeter {
    /// Peak (maximum absolute) level.
    pub peak_dbfs: f32,
    /// RMS (root-mean-square) level.
    pub rms_dbfs: f32,
}

/// Measure the peak and RMS level of `samples` in dBFS. An empty buffer reads as silence.
pub fn meter(samples: &[f32]) -> LevelMeter {
    if samples.is_empty() {
        return LevelMeter {
            peak_dbfs: SILENCE_DBFS,
            rms_dbfs: SILENCE_DBFS,
        };
    }
    let peak = samples.iter().fold(0.0f32, |max, &s| max.max(s.abs()));
    let mean_square = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    LevelMeter {
        peak_dbfs: linear_to_dbfs(peak),
        rms_dbfs: linear_to_dbfs(mean_square.sqrt()),
    }
}

/// Apply a linear fade-in over the first `samples` of `buffer` in place: the gain ramps
/// from 0 to 1 (clamped to the buffer length).
pub fn apply_fade_in(buffer: &mut [f32], samples: usize) {
    let count = samples.min(buffer.len());
    if count <= 1 {
        return;
    }
    for (index, sample) in buffer.iter_mut().take(count).enumerate() {
        *sample *= index as f32 / (count - 1) as f32;
    }
}

/// Apply a linear fade-out over the last `samples` of `buffer` in place: the gain ramps
/// from 1 down to 0 (clamped to the buffer length).
pub fn apply_fade_out(buffer: &mut [f32], samples: usize) {
    let length = buffer.len();
    let count = samples.min(length);
    if count <= 1 {
        return;
    }
    for offset in 0..count {
        buffer[length - 1 - offset] *= offset as f32 / (count - 1) as f32;
    }
}

/// Equal-power crossfade from `outgoing` to `incoming` across the overlap (the shorter of
/// the two lengths): the outgoing gain follows `cos`, the incoming `sin`, keeping constant
/// power through the transition. The audio clip-transition mix.
pub fn crossfade(outgoing: &[f32], incoming: &[f32]) -> Vec<f32> {
    let length = outgoing.len().min(incoming.len());
    let mut output = Vec::with_capacity(length);
    for index in 0..length {
        let t = if length <= 1 {
            0.0
        } else {
            index as f32 / (length - 1) as f32
        };
        let angle = t * std::f32::consts::FRAC_PI_2;
        output.push(outgoing[index] * angle.cos() + incoming[index] * angle.sin());
    }
    output
}

/// Scale every sample in `buffer` by `gain` in place.
pub fn apply_gain(buffer: &mut [f32], gain: f32) {
    for sample in buffer.iter_mut() {
        *sample *= gain;
    }
}

/// The linear gain that scales `samples` so their peak magnitude reaches `target_dbfs`.
/// Returns `1.0` for silence (nothing to normalize). Combine with [`apply_gain`] to
/// peak-normalize a buffer.
pub fn peak_normalize_gain(samples: &[f32], target_dbfs: f32) -> f32 {
    let peak = samples
        .iter()
        .fold(0.0f32, |max, &sample| max.max(sample.abs()));
    if peak <= 0.0 {
        return 1.0;
    }
    dbfs_to_linear(target_dbfs) / peak
}

/// The linear gain that scales `samples` so their RMS level reaches `target_dbfs`.
/// Returns `1.0` for silence. Combine with [`apply_gain`] to loudness-normalize a buffer to
/// a target RMS (a simpler stand-in for full BS.1770 LUFS normalization).
pub fn rms_normalize_gain(samples: &[f32], target_dbfs: f32) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }
    let mean_square =
        samples.iter().map(|&sample| sample * sample).sum::<f32>() / samples.len() as f32;
    let rms = mean_square.sqrt();
    if rms <= 0.0 {
        return 1.0;
    }
    dbfs_to_linear(target_dbfs) / rms
}

struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn process(&self, input: &[f32]) -> Vec<f32> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        input
            .iter()
            .map(|&sample| {
                let x0 = sample as f64;
                let y0 = self.b0 * x0 + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
                x2 = x1;
                x1 = x0;
                y2 = y1;
                y1 = y0;
                y0 as f32
            })
            .collect()
    }
}

fn high_pass(sample_rate: f64, fc: f64, q: f64) -> Biquad {
    let w0 = 2.0 * std::f64::consts::PI * fc / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    Biquad {
        b0: ((1.0 + cos_w0) / 2.0) / a0,
        b1: -(1.0 + cos_w0) / a0,
        b2: ((1.0 + cos_w0) / 2.0) / a0,
        a1: (-2.0 * cos_w0) / a0,
        a2: (1.0 - alpha) / a0,
    }
}

fn high_shelf(sample_rate: f64, fc: f64, gain_db: f64, q: f64) -> Biquad {
    let amp = 10.0_f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * fc / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let shelf = 2.0 * amp.sqrt() * alpha;
    let a0 = (amp + 1.0) - (amp - 1.0) * cos_w0 + shelf;
    Biquad {
        b0: (amp * ((amp + 1.0) + (amp - 1.0) * cos_w0 + shelf)) / a0,
        b1: (-2.0 * amp * ((amp - 1.0) + (amp + 1.0) * cos_w0)) / a0,
        b2: (amp * ((amp + 1.0) + (amp - 1.0) * cos_w0 - shelf)) / a0,
        a1: (2.0 * ((amp - 1.0) - (amp + 1.0) * cos_w0)) / a0,
        a2: ((amp + 1.0) - (amp - 1.0) * cos_w0 - shelf) / a0,
    }
}

/// The ITU-R BS.1770 "K-weighting" two-stage prefilter (high-shelf then high-pass) at
/// `sample_rate`, used before measuring loudness.
fn k_weighting(sample_rate: u32) -> (Biquad, Biquad) {
    let fs = sample_rate as f64;
    (
        high_shelf(fs, 1681.974450955533, 3.999843853973347, 0.7071752369554196),
        high_pass(fs, 38.13547087613982, 0.5003270373238773),
    )
}

/// Momentary loudness of a mono `samples` buffer in **LUFS** (ITU-R BS.1770 K-weighting):
/// `-0.691 + 10*log10(mean_square)` of the K-weighted signal. This is the broadcast
/// loudness measure behind LUFS metering (the gated *integrated* loudness adds block gating
/// on top). Silence (or a zero sample rate) returns [`SILENCE_DBFS`].
pub fn momentary_lufs(samples: &[f32], sample_rate: u32) -> f32 {
    if samples.is_empty() || sample_rate == 0 {
        return SILENCE_DBFS;
    }
    let (shelf, hpf) = k_weighting(sample_rate);
    let weighted = hpf.process(&shelf.process(samples));
    let mean_square =
        weighted.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / weighted.len() as f64;
    if mean_square <= 0.0 {
        return SILENCE_DBFS;
    }
    (-0.691 + 10.0 * mean_square.log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{TimelineClip, TimelineTrack};

    struct ConstProvider {
        value: f32,
    }

    impl AudioProvider for ConstProvider {
        fn samples(&self, _source: &str, _source_frame: u64, spf: usize) -> Option<Vec<f32>> {
            Some(vec![self.value; spf])
        }
    }

    fn clip(source: &str, start: u64, end: u64, offset: u64) -> TimelineClip {
        TimelineClip {
            id: source.to_string(),
            source: source.to_string(),
            start_frame: start,
            end_frame: end,
            track_offset: offset,
            opacity: 1.0,
            blend_mode: Default::default(),
            effects: Default::default(),
        }
    }

    fn track(track_type: TrackType, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: "t".to_string(),
            name: "t".to_string(),
            track_type,
            clips,
            enabled: true,
        }
    }

    fn timeline(tracks: Vec<TimelineTrack>, fps: f64) -> Timeline {
        Timeline {
            tracks,
            frame_rate: fps,
            duration_frames: 100,
        }
    }

    #[test]
    fn mixes_one_audio_track_at_correct_length() {
        let tl = timeline(
            vec![track(TrackType::Audio, vec![clip("a", 0, 4, 0)])],
            30.0,
        );
        let out = mix_range(&tl, 0..2, 48_000, &ConstProvider { value: 0.5 }).unwrap();
        // 30 fps @ 48k = 1600 samples/frame, two frames -> 3200 samples, all 0.5.
        assert_eq!(out.len(), 3200);
        assert!(out.iter().all(|sample| (sample - 0.5).abs() < 1e-6));
    }

    #[test]
    fn sums_overlapping_audio_tracks() {
        let tl = timeline(
            vec![
                track(TrackType::Audio, vec![clip("a", 0, 4, 0)]),
                track(TrackType::Audio, vec![clip("b", 0, 4, 0)]),
            ],
            30.0,
        );
        let out = mix_range(&tl, 0..1, 48_000, &ConstProvider { value: 0.5 }).unwrap();
        // Two overlapping audio tracks sum: 0.5 + 0.5 = 1.0.
        assert!(out.iter().all(|sample| (sample - 1.0).abs() < 1e-6));
    }

    #[test]
    fn disabled_track_is_muted_in_the_mix() {
        let mut tl = timeline(
            vec![
                track(TrackType::Audio, vec![clip("a", 0, 4, 0)]),
                track(TrackType::Audio, vec![clip("b", 0, 4, 0)]),
            ],
            30.0,
        );
        tl.tracks[1].enabled = false;
        let out = mix_range(&tl, 0..1, 48_000, &ConstProvider { value: 0.5 }).unwrap();
        // The muted second track contributes nothing: only 0.5 remains.
        assert!(out.iter().all(|sample| (sample - 0.5).abs() < 1e-6));
    }

    #[test]
    fn ignores_non_audio_tracks() {
        let tl = timeline(
            vec![
                track(TrackType::Video, vec![clip("v", 0, 4, 0)]),
                track(TrackType::Audio, vec![clip("a", 0, 4, 0)]),
            ],
            30.0,
        );
        let out = mix_range(&tl, 0..1, 48_000, &ConstProvider { value: 0.5 }).unwrap();
        // Only the audio track contributes -> 0.5, not 1.0.
        assert!(out.iter().all(|sample| (sample - 0.5).abs() < 1e-6));
    }

    #[test]
    fn silence_where_no_clip_is_active() {
        let tl = timeline(
            vec![track(TrackType::Audio, vec![clip("a", 0, 2, 0)])],
            30.0,
        );
        // Frame 5 has no clip -> silence.
        let out = mix_range(&tl, 5..6, 48_000, &ConstProvider { value: 0.5 }).unwrap();
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn rejects_non_integer_samples_per_frame() {
        let tl = timeline(
            vec![track(TrackType::Audio, vec![clip("a", 0, 4, 0)])],
            30.0,
        );
        assert!(mix_range(&tl, 0..1, 48_001, &ConstProvider { value: 0.5 }).is_none());
        let bad = timeline(vec![], 0.0);
        assert!(mix_range(&bad, 0..1, 48_000, &ConstProvider { value: 0.5 }).is_none());
    }

    #[test]
    fn is_deterministic() {
        let tl = timeline(
            vec![track(TrackType::Audio, vec![clip("a", 0, 10, 0)])],
            24.0,
        );
        let run = || mix_range(&tl, 0..5, 48_000, &ConstProvider { value: 0.3 }).unwrap();
        assert_eq!(run(), run());
    }

    #[test]
    fn wav_provider_decodes_and_feeds_the_mixdown() {
        use crate::export::encode_wav_pcm16;
        // Two frames of constant 0.5 at 48k/30fps (1600 samples/frame).
        let wav = encode_wav_pcm16(&vec![0.5f32; 3200], 48_000, 1);
        let mut provider = WavAudioProvider::new();
        assert!(provider.add_wav("clip.wav", &wav));
        assert!(!provider.add_wav("bad", b"not a wav"));

        let tl = timeline(
            vec![track(TrackType::Audio, vec![clip("clip.wav", 0, 4, 0)])],
            30.0,
        );
        let out = mix_range(&tl, 0..2, 48_000, &provider).unwrap();
        assert_eq!(out.len(), 3200);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-3), "decoded ~0.5");
    }

    #[test]
    fn pan_gains_follow_the_law() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;

        // Center: linear -6 dB, constant-power -3 dB, compromise -4.5 dB.
        let (left, right) = pan_gains(0.0, PanLaw::Linear);
        assert!(close(left, 0.5) && close(right, 0.5));
        let (left, right) = pan_gains(0.0, PanLaw::ConstantPower);
        assert!(close(left, 0.70711) && close(right, 0.70711));
        let (left, right) = pan_gains(0.0, PanLaw::Minus4_5dB);
        assert!(close(left, (0.5_f32 * 0.70711).sqrt()) && close(left, right));

        // Hard left / right.
        let (left, right) = pan_gains(-1.0, PanLaw::ConstantPower);
        assert!(close(left, 1.0) && close(right, 0.0));
        let (left, right) = pan_gains(1.0, PanLaw::Linear);
        assert!(close(left, 0.0) && close(right, 1.0));

        // Pan is clamped to the valid range.
        assert_eq!(
            pan_gains(5.0, PanLaw::Linear),
            pan_gains(1.0, PanLaw::Linear)
        );
    }

    #[test]
    fn dbfs_conversions_and_level_metering() {
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(close(linear_to_dbfs(1.0), 0.0));
        assert!(close(linear_to_dbfs(0.5), -6.0206));
        assert_eq!(linear_to_dbfs(0.0), SILENCE_DBFS);
        assert!(close(dbfs_to_linear(0.0), 1.0));
        assert!(close(dbfs_to_linear(-6.0206), 0.5));

        // Constant ±0.5 buffer: peak == rms == 0.5 -> ~-6 dBFS.
        let level = meter(&[0.5, -0.5, 0.5, -0.5]);
        assert!(close(level.peak_dbfs, -6.0206));
        assert!(close(level.rms_dbfs, -6.0206));

        // A single full-scale spike: peak 0 dBFS, RMS below it.
        let level = meter(&[1.0, 0.0, 0.0, 0.0]);
        assert!(close(level.peak_dbfs, 0.0));
        assert!(level.rms_dbfs < level.peak_dbfs);

        // Empty buffer reads as silence.
        assert_eq!(meter(&[]).peak_dbfs, SILENCE_DBFS);
    }

    #[test]
    fn fades_ramp_the_buffer_edges() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;

        let mut fade_in = vec![1.0f32; 4];
        apply_fade_in(&mut fade_in, 4);
        assert!(
            close(fade_in[0], 0.0)
                && close(fade_in[1], 1.0 / 3.0)
                && close(fade_in[2], 2.0 / 3.0)
                && close(fade_in[3], 1.0)
        );

        let mut fade_out = vec![1.0f32; 4];
        apply_fade_out(&mut fade_out, 4);
        assert!(
            close(fade_out[0], 1.0)
                && close(fade_out[1], 2.0 / 3.0)
                && close(fade_out[2], 1.0 / 3.0)
                && close(fade_out[3], 0.0)
        );

        // A fade longer than the buffer clamps to its length.
        let mut short = vec![1.0f32; 2];
        apply_fade_in(&mut short, 100);
        assert!(close(short[0], 0.0) && close(short[1], 1.0));
    }

    #[test]
    fn crossfade_blends_outgoing_to_incoming() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let result = crossfade(&[1.0; 3], &[0.0; 3]);
        // Starts full outgoing, ends full incoming (0 here), equal-power midpoint.
        assert!(close(result[0], 1.0));
        assert!(close(result[2], 0.0));
        assert!(close(result[1], std::f32::consts::FRAC_1_SQRT_2));

        // Identical sources follow cos+sin (constant-power law).
        let same = crossfade(&[1.0; 3], &[1.0; 3]);
        for (index, &value) in same.iter().enumerate() {
            let angle = index as f32 / 2.0 * std::f32::consts::FRAC_PI_2;
            assert!(close(value, angle.cos() + angle.sin()));
        }

        // Mismatched lengths use the shorter.
        assert_eq!(crossfade(&[1.0, 1.0], &[0.0]).len(), 1);
    }

    #[test]
    fn apply_gain_scales_every_sample() {
        let mut buffer = vec![0.5, -0.25, 0.0];
        apply_gain(&mut buffer, 2.0);
        assert_eq!(buffer, vec![1.0, -0.5, 0.0]);
    }

    #[test]
    fn peak_normalize_brings_the_peak_to_target() {
        let mut buffer = vec![0.25, -0.5, 0.1];
        // 0 dBFS == linear 1.0; the peak of 0.5 needs 2x to reach it.
        let gain = peak_normalize_gain(&buffer, 0.0);
        assert!((gain - 2.0).abs() < 1e-6);
        apply_gain(&mut buffer, gain);
        let peak = buffer.iter().fold(0.0f32, |max, &s| max.max(s.abs()));
        assert!((peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rms_normalize_brings_the_rms_to_target() {
        // A square-ish signal of amplitude 0.5 has RMS 0.5; normalizing to 0 dBFS doubles it.
        let mut buffer = vec![0.5, -0.5, 0.5, -0.5];
        let gain = rms_normalize_gain(&buffer, 0.0);
        assert!((gain - 2.0).abs() < 1e-6);
        apply_gain(&mut buffer, gain);
        assert_eq!(meter(&buffer).rms_dbfs.round(), 0.0);
    }

    #[test]
    fn normalize_gains_are_unity_for_silence() {
        assert_eq!(peak_normalize_gain(&[0.0, 0.0], 0.0), 1.0);
        assert_eq!(peak_normalize_gain(&[], -3.0), 1.0);
        assert_eq!(rms_normalize_gain(&[0.0, 0.0], 0.0), 1.0);
        assert_eq!(rms_normalize_gain(&[], -3.0), 1.0);
    }

    fn tone(amplitude: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|n| (n as f32 * 0.13).sin() * amplitude)
            .collect()
    }

    #[test]
    fn momentary_lufs_rises_about_6db_when_amplitude_doubles() {
        // The K-weighting filter is linear, so doubling amplitude is 4x power = +6.02 LU,
        // independent of the exact coefficients.
        let quiet = momentary_lufs(&tone(0.25, 4800), 48_000);
        let loud = momentary_lufs(&tone(0.5, 4800), 48_000);
        assert!((loud - quiet - 6.0206).abs() < 0.05, "{quiet} -> {loud}");
    }

    #[test]
    fn momentary_lufs_rejects_dc_offset() {
        // A constant offset is removed by the K-weighting high-pass, so it reads far
        // quieter than an equal-amplitude tone.
        let dc = momentary_lufs(&vec![0.5f32; 48_000], 48_000);
        let tone = momentary_lufs(&tone(0.5, 48_000), 48_000);
        assert!(dc < tone - 20.0, "dc {dc} vs tone {tone}");
    }

    #[test]
    fn momentary_lufs_is_monotonic_and_floors_at_silence() {
        assert_eq!(momentary_lufs(&[0.0; 1000], 48_000), SILENCE_DBFS);
        assert_eq!(momentary_lufs(&[], 48_000), SILENCE_DBFS);
        assert_eq!(momentary_lufs(&tone(0.4, 1000), 0), SILENCE_DBFS);
        assert!(
            momentary_lufs(&tone(0.2, 4800), 48_000) < momentary_lufs(&tone(0.4, 4800), 48_000)
        );
    }
}
