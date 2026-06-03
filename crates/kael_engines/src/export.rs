//! Timeline render/export pass — drives [`composite_frame`] across a frame range and
//! feeds each composited frame to an [`ExportSink`] with its presentation timestamp.
//!
//! This is the output-side counterpart to [`crate::compositor::FrameProvider`]: the sink
//! is the encoder/muxer in production and a collector in tests, so the full
//! timeline→composited-frame-sequence path runs deterministically without an encoder.

use std::ops::Range;
use std::time::Duration;

use kael_render_graph::reference::Image;

use crate::audio_mix::{mix_range, AudioProvider};
use crate::compositor::{composite_frame, FrameProvider};
use crate::media::Timeline;

/// Receives each composited output frame of a render/export pass.
pub trait ExportSink {
    /// Handle the frame at `frame_index` with presentation time `pts`.
    fn frame(&mut self, frame_index: u64, pts: Duration, image: &Image);
}

/// Render every frame in `range` of `timeline` through [`composite_frame`], feeding each
/// to `sink` with the presentation timestamp from the timeline's timebase (zero if the
/// timeline's frame rate is invalid). Returns the number of frames rendered.
///
/// Rendering is deterministic: the same timeline, range, and provider always produce the
/// same frame sequence — the property export determinism requires.
pub fn render_range(
    timeline: &Timeline,
    range: Range<u64>,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
    sink: &mut dyn ExportSink,
) -> usize {
    let timebase = timeline.timebase();
    let mut rendered = 0;
    for frame in range {
        let image = composite_frame(timeline, frame, width, height, provider);
        let pts = timebase
            .map(|tb| tb.frame_to_time(frame))
            .unwrap_or(Duration::ZERO);
        sink.frame(frame, pts, &image);
        rendered += 1;
    }
    rendered
}

/// Encode an image as a binary PPM (P6) — an uncompressed RGB frame, the dependency-free
/// image-sequence export target. Linear `f32` channels are clamped to `0..=1` and
/// quantized to 8-bit; alpha is dropped (PPM has no alpha channel).
pub fn encode_ppm(image: &Image) -> Vec<u8> {
    let mut bytes = format!("P6\n{} {}\n255\n", image.width, image.height).into_bytes();
    bytes.reserve(image.pixels.len() * 3);
    for pixel in &image.pixels {
        for channel in &pixel[0..3] {
            bytes.push((channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    bytes
}

fn next_ppm_uint(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    loop {
        while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
        if *pos < bytes.len() && bytes[*pos] == b'#' {
            while *pos < bytes.len() && bytes[*pos] != b'\n' {
                *pos += 1;
            }
        } else {
            break;
        }
    }
    let start = *pos;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    std::str::from_utf8(&bytes[start..*pos]).ok()?.parse().ok()
}

/// Decode a binary PPM (P6) into an [`Image`] (8-bit RGB → `f32`, alpha 1.0) — the decode
/// counterpart to [`encode_ppm`]. Tolerates header comments and whitespace; returns
/// `None` on malformed input or a non-255 max value.
pub fn decode_ppm(bytes: &[u8]) -> Option<Image> {
    if bytes.len() < 2 || &bytes[0..2] != b"P6" {
        return None;
    }
    let mut pos = 2;
    let width = next_ppm_uint(bytes, &mut pos)?;
    let height = next_ppm_uint(bytes, &mut pos)?;
    let maxval = next_ppm_uint(bytes, &mut pos)?;
    if maxval != 255 {
        return None;
    }
    // Exactly one whitespace byte separates the max value from the binary data.
    pos += 1;
    let pixel_count = (width as usize).checked_mul(height as usize)?;
    let data = bytes.get(pos..pos.checked_add(pixel_count * 3)?)?;
    let mut image = Image::new(width, height);
    image.pixels = data
        .chunks_exact(3)
        .map(|rgb| {
            [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
                1.0,
            ]
        })
        .collect();
    Some(image)
}

/// Encode interleaved `samples` as a 16-bit PCM WAV (RIFF/WAVE) file — the dependency-free
/// audio export target, the [`mix_range`](crate::audio_mix::mix_range) counterpart to
/// [`encode_ppm`]. Float samples are clamped to `-1.0..=1.0` and quantized to `i16`.
pub fn encode_wav_pcm16(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * block_align as u32;
    let data_size = (samples.len() * 2) as u32;

    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for &sample in samples {
        let quantized = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        bytes.extend_from_slice(&quantized.to_le_bytes());
    }
    bytes
}

/// Decode a 16-bit PCM WAV file into `(samples, sample_rate, channels)` — the decode
/// counterpart to [`encode_wav_pcm16`] and a concrete uncompressed [`AudioProvider`]
/// source. Walks the RIFF chunks, requiring a PCM `fmt ` chunk and a `data` chunk.
/// Returns `None` on malformed input or an unsupported format.
pub fn decode_wav_pcm16(bytes: &[u8]) -> Option<(Vec<f32>, u32, u16)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12;
    let mut format: Option<(u16, u32)> = None;
    let mut samples: Option<Vec<f32>> = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let body_start = offset + 8;
        let body_end = body_start.checked_add(size)?;
        if body_end > bytes.len() {
            return None;
        }
        let body = &bytes[body_start..body_end];
        if id == b"fmt " {
            if body.len() < 16 {
                return None;
            }
            let audio_format = u16::from_le_bytes([body[0], body[1]]);
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            if audio_format != 1 || bits != 16 {
                return None;
            }
            format = Some((channels, sample_rate));
        } else if id == b"data" {
            samples = Some(
                body.chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32767.0)
                    .collect(),
            );
        }
        // RIFF chunks are word-aligned: skip a pad byte after an odd-sized chunk.
        offset = body_end + (size & 1);
    }
    let (channels, sample_rate) = format?;
    Some((samples?, sample_rate, channels))
}

/// A fully exported timeline segment: one PPM video frame per timeline frame plus a single
/// WAV audio track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedTimeline {
    /// PPM (P6) bytes, one per frame in the rendered range.
    pub frames: Vec<Vec<u8>>,
    /// The mixed audio track as a mono PCM-16 WAV.
    pub audio: Vec<u8>,
}

/// Render a timeline `range` to uncompressed PPM video frames and a WAV audio track — the
/// top-level export, composing [`composite_frame`] + [`encode_ppm`] for each frame and
/// [`mix_range`] + [`encode_wav_pcm16`] for audio. Deterministic given the same inputs.
#[allow(clippy::too_many_arguments)]
pub fn export_timeline(
    timeline: &Timeline,
    range: Range<u64>,
    width: u32,
    height: u32,
    sample_rate: u32,
    video: &dyn FrameProvider,
    audio: &dyn AudioProvider,
) -> ExportedTimeline {
    let frames = range
        .clone()
        .map(|frame| encode_ppm(&composite_frame(timeline, frame, width, height, video)))
        .collect();
    let samples = mix_range(timeline, range, sample_rate, audio).unwrap_or_default();
    ExportedTimeline {
        frames,
        audio: encode_wav_pcm16(&samples, sample_rate, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::GeneratorProvider;
    use crate::media::{Timeline, TimelineClip, TimelineTrack, TrackType};

    struct CollectingSink {
        frames: Vec<(u64, Duration, [f32; 4])>,
    }

    impl ExportSink for CollectingSink {
        fn frame(&mut self, frame_index: u64, pts: Duration, image: &Image) {
            self.frames.push((frame_index, pts, image.pixel(0, 0)));
        }
    }

    fn clip(id: &str, source: &str, start: u64, end: u64, offset: u64) -> TimelineClip {
        TimelineClip {
            id: id.to_string(),
            source: source.to_string(),
            start_frame: start,
            end_frame: end,
            track_offset: offset,
            opacity: 1.0,
            opacity_curve: None,
            blend_mode: Default::default(),
            effects: Default::default(),
            transform: Default::default(),
            transform_curve: None,
        }
    }

    #[test]
    fn render_range_is_frame_accurate_and_timestamped() {
        // Two back-to-back generator clips on one track: red over [0,2), blue over [2,4).
        let timeline = Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "v1".to_string(),
                track_type: TrackType::Video,
                enabled: true,
                clips: vec![
                    clip("a", "color:1,0,0,1", 0, 2, 0),
                    clip("b", "color:0,0,1,1", 0, 2, 2),
                ],
            }],
            frame_rate: 30.0,
            duration_frames: 4,
        };

        let mut sink = CollectingSink { frames: Vec::new() };
        let rendered = render_range(&timeline, 0..4, 1, 1, &GeneratorProvider, &mut sink);

        assert_eq!(rendered, 4);
        assert_eq!(sink.frames.len(), 4);
        // Frame-accurate clip switch at frame 2.
        assert_eq!(sink.frames[0].2, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(sink.frames[1].2, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(sink.frames[2].2, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(sink.frames[3].2, [0.0, 0.0, 1.0, 1.0]);
        // Indices and presentation timestamps follow the timebase (30 fps).
        assert_eq!(sink.frames[0].0, 0);
        assert_eq!(sink.frames[3].0, 3);
        assert_eq!(sink.frames[0].1, Duration::ZERO);
        assert!(sink.frames[1].1 > Duration::ZERO);
        // 30 fps: frame 3 lands at ~100ms.
        assert!((sink.frames[3].1.as_secs_f64() - 0.1).abs() < 1e-3);
    }

    #[test]
    fn render_range_is_deterministic() {
        let timeline = Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "v1".to_string(),
                track_type: TrackType::Video,
                enabled: true,
                clips: vec![clip("a", "gradient:0,0,0,1|1,1,1,1|h", 0, 10, 0)],
            }],
            frame_rate: 24.0,
            duration_frames: 10,
        };
        let collect = || {
            let mut sink = CollectingSink { frames: Vec::new() };
            render_range(&timeline, 0..5, 4, 4, &GeneratorProvider, &mut sink);
            sink.frames
        };
        assert_eq!(collect(), collect());
    }

    #[test]
    fn encode_ppm_writes_p6_header_and_rgb_bytes() {
        let mut image = Image::new(2, 1);
        image.pixels = vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]];
        let ppm = encode_ppm(&image);
        let header = b"P6\n2 1\n255\n";
        assert!(ppm.starts_with(header));
        // Red then green, 3 bytes per pixel, alpha dropped.
        assert_eq!(&ppm[header.len()..], &[255, 0, 0, 0, 255, 0]);
        assert_eq!(ppm.len(), header.len() + 6);
    }

    #[test]
    fn encode_ppm_clamps_and_quantizes() {
        let mut image = Image::new(1, 1);
        image.pixels = vec![[1.5, -0.5, 0.5, 1.0]];
        let ppm = encode_ppm(&image);
        // 1.5 -> 255, -0.5 -> 0, 0.5 -> round(127.5) = 128.
        assert_eq!(&ppm[ppm.len() - 3..], &[255, 0, 128]);
    }

    #[test]
    fn ppm_round_trips_through_encode_decode() {
        let mut image = Image::new(2, 1);
        image.pixels = vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.5, 1.0, 1.0]];
        let decoded = decode_ppm(&encode_ppm(&image)).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        for (got, want) in decoded.pixels.iter().zip(&image.pixels) {
            for channel in 0..4 {
                assert!(
                    (got[channel] - want[channel]).abs() < 1.0 / 255.0 + 1e-6,
                    "{got:?} vs {want:?}"
                );
            }
        }
        // Header comments and whitespace are tolerated.
        let commented = b"P6\n# a comment\n2 1\n255\n\xff\x00\x00\x00\x80\xff";
        assert!(decode_ppm(commented).is_some());
        // Malformed input is rejected.
        assert!(decode_ppm(b"P3 plain ppm").is_none());
        assert!(decode_ppm(&encode_ppm(&image)[..5]).is_none());
    }

    #[test]
    fn render_range_to_ppm_produces_valid_frames() {
        let timeline = Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "v1".to_string(),
                track_type: TrackType::Video,
                enabled: true,
                clips: vec![clip("a", "color:1,0,0,1", 0, 4, 0)],
            }],
            frame_rate: 30.0,
            duration_frames: 4,
        };
        struct PpmSink {
            files: Vec<Vec<u8>>,
        }
        impl ExportSink for PpmSink {
            fn frame(&mut self, _index: u64, _pts: Duration, image: &Image) {
                self.files.push(encode_ppm(image));
            }
        }
        let mut sink = PpmSink { files: Vec::new() };
        render_range(&timeline, 0..3, 2, 2, &GeneratorProvider, &mut sink);
        assert_eq!(sink.files.len(), 3);
        for file in &sink.files {
            assert!(file.starts_with(b"P6\n2 2\n255\n"));
            // 2x2 red frame: every pixel is 255,0,0.
            assert!(file.ends_with(&[255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0]));
        }
    }

    #[test]
    fn encode_wav_pcm16_writes_riff_header_and_quantized_samples() {
        let wav = encode_wav_pcm16(&[0.0, 1.0, -1.0], 48_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        // 44-byte header + 3 samples * 2 bytes.
        assert_eq!(wav.len(), 50);
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
        // fmt fields: channels = 1, sample rate = 48000.
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            48_000
        );
        // Samples: 0 -> 0, 1.0 -> 32767, -1.0 -> -32767.
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), 0);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), 32767);
        assert_eq!(i16::from_le_bytes([wav[48], wav[49]]), -32767);
    }

    #[test]
    fn wav_pcm16_round_trips_through_encode_decode() {
        let samples = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let wav = encode_wav_pcm16(&samples, 44_100, 2);
        let (decoded, sample_rate, channels) = decode_wav_pcm16(&wav).unwrap();
        assert_eq!(sample_rate, 44_100);
        assert_eq!(channels, 2);
        assert_eq!(decoded.len(), samples.len());
        for (decoded_sample, original) in decoded.iter().zip(&samples) {
            assert!(
                (decoded_sample - original).abs() < 1e-4,
                "{decoded_sample} vs {original}"
            );
        }
        // Malformed input is rejected.
        assert!(decode_wav_pcm16(b"not a wav file at all").is_none());
        assert!(decode_wav_pcm16(&wav[..10]).is_none());
    }

    #[test]
    fn export_timeline_produces_ppm_frames_and_wav_audio() {
        use crate::audio_mix::WavAudioProvider;

        let timeline = Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "v1".to_string(),
                track_type: TrackType::Video,
                enabled: true,
                clips: vec![clip("a", "color:0,1,0,1", 0, 30, 0)],
            }],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let exported = export_timeline(
            &timeline,
            0..2,
            2,
            2,
            48_000,
            &GeneratorProvider,
            &WavAudioProvider::new(),
        );

        // Two green PPM frames.
        assert_eq!(exported.frames.len(), 2);
        for frame in &exported.frames {
            assert!(frame.starts_with(b"P6\n2 2\n255\n"));
            assert!(frame.ends_with(&[0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0]));
        }
        // WAV audio: RIFF/WAVE header, 2 frames * 1600 samples * 2 bytes = 6400 data bytes.
        assert!(exported.audio.starts_with(b"RIFF"));
        assert_eq!(&exported.audio[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes([
                exported.audio[40],
                exported.audio[41],
                exported.audio[42],
                exported.audio[43]
            ]),
            6400
        );
    }
}
