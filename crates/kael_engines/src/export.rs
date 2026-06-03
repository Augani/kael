//! Timeline render/export pass — drives [`composite_frame`] across a frame range and
//! feeds each composited frame to an [`ExportSink`] with its presentation timestamp.
//!
//! This is the output-side counterpart to [`crate::compositor::FrameProvider`]: the sink
//! is the encoder/muxer in production and a collector in tests, so the full
//! timeline→composited-frame-sequence path runs deterministically without an encoder.

use std::ops::Range;
use std::time::Duration;

use kael_render_graph::reference::Image;

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
            blend_mode: Default::default(),
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
    fn render_range_to_ppm_produces_valid_frames() {
        let timeline = Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "v1".to_string(),
                track_type: TrackType::Video,
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
}
