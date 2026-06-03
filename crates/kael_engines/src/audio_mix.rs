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
            if track.track_type != TrackType::Audio {
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
        }
    }

    fn track(track_type: TrackType, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: "t".to_string(),
            name: "t".to_string(),
            track_type,
            clips,
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
}
