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
}
