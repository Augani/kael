//! Timeline-driven compositing against the reference compositor — the preview==export
//! oracle (V1 + V5).
//!
//! [`composite_frame`] resolves which source frame each track shows at a given timeline
//! frame (via [`Timeline::frame_requests`]) and stacks the tracks bottom-to-top with
//! source-over alpha compositing. Source images come from a [`FrameProvider`]: the media
//! decoder in production, a synthetic generator in tests — so the full timeline→pixels
//! path is exercised without a decoder.

use kael_render_graph::reference::{blend_over, Image};

use crate::media::Timeline;

/// Supplies a decoded source image for a clip's source frame.
///
/// Returning `None` means the frame is unavailable, and that layer is skipped rather
/// than aborting the whole composite.
pub trait FrameProvider {
    /// Fetch the image for `source` at `source_frame`, sized `width` x `height`.
    fn frame(&self, source: &str, source_frame: u64, width: u32, height: u32) -> Option<Image>;
}

/// Composite the timeline at `frame` into a single `width` x `height` image.
///
/// Each track's active clip is fetched from `provider` and stacked in track-declaration
/// order (the first track is the bottom layer) with source-over compositing. Tracks with
/// no active clip — or whose source frame the provider cannot supply, or whose supplied
/// image is the wrong size — are skipped. The result is transparent where nothing is active.
pub fn composite_frame(
    timeline: &Timeline,
    frame: u64,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
) -> Image {
    let mut output = Image::new(width, height);
    for request in timeline.frame_requests(frame) {
        let Some(layer) = provider.frame(&request.source, request.source_frame, width, height)
        else {
            continue;
        };
        if layer.width != width || layer.height != height {
            continue;
        }
        let mut next = Image::new(width, height);
        blend_over(&[&layer, &output], &mut next);
        output = next;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{TimelineClip, TimelineTrack, TrackType};
    use std::collections::HashMap;

    struct SolidProvider {
        colors: HashMap<String, [f32; 4]>,
    }

    impl FrameProvider for SolidProvider {
        fn frame(
            &self,
            source: &str,
            _source_frame: u64,
            width: u32,
            height: u32,
        ) -> Option<Image> {
            self.colors
                .get(source)
                .map(|&color| Image::filled(width, height, color))
        }
    }

    fn clip(id: &str, source: &str, start: u64, end: u64, offset: u64) -> TimelineClip {
        TimelineClip {
            id: id.to_string(),
            source: source.to_string(),
            start_frame: start,
            end_frame: end,
            track_offset: offset,
        }
    }

    fn track(id: &str, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: id.to_string(),
            name: id.to_string(),
            track_type: TrackType::Video,
            clips,
        }
    }

    fn provider(pairs: &[(&str, [f32; 4])]) -> SolidProvider {
        SolidProvider {
            colors: pairs
                .iter()
                .map(|(source, color)| (source.to_string(), *color))
                .collect(),
        }
    }

    #[test]
    fn composites_single_opaque_track() {
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "red", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let out = composite_frame(
            &timeline,
            10,
            2,
            2,
            &provider(&[("red", [1.0, 0.0, 0.0, 1.0])]),
        );
        assert!(out
            .pixels
            .iter()
            .all(|pixel| *pixel == [1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn upper_track_composites_over_lower() {
        let timeline = Timeline {
            tracks: vec![
                track("v1", vec![clip("a", "red", 0, 30, 0)]),
                track("v2", vec![clip("b", "blue50", 0, 30, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let provider = provider(&[
            ("red", [1.0, 0.0, 0.0, 1.0]),
            ("blue50", [0.0, 0.0, 1.0, 0.5]),
        ]);
        let pixel = composite_frame(&timeline, 5, 1, 1, &provider).pixel(0, 0);
        // Semi-transparent blue over opaque red: out_a=1, rgb = blue*0.5 + red*0.5.
        assert!((pixel[0] - 0.5).abs() < 1e-5, "red {}", pixel[0]);
        assert!((pixel[2] - 0.5).abs() < 1e-5, "blue {}", pixel[2]);
        assert!((pixel[3] - 1.0).abs() < 1e-5, "alpha {}", pixel[3]);
    }

    #[test]
    fn source_frame_passed_to_provider_is_frame_accurate() {
        // Clip source [100,110) placed at track offset 50; at timeline frame 53 the
        // provider must be asked for source frame 103.
        struct RecordingProvider {
            requested: std::cell::Cell<u64>,
        }
        impl FrameProvider for RecordingProvider {
            fn frame(&self, _s: &str, source_frame: u64, w: u32, h: u32) -> Option<Image> {
                self.requested.set(source_frame);
                Some(Image::new(w, h))
            }
        }
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "s", 100, 110, 50)])],
            frame_rate: 30.0,
            duration_frames: 200,
        };
        let provider = RecordingProvider {
            requested: std::cell::Cell::new(0),
        };
        composite_frame(&timeline, 53, 1, 1, &provider);
        assert_eq!(provider.requested.get(), 103);
    }

    #[test]
    fn empty_timeline_frame_is_transparent() {
        let timeline = Timeline {
            tracks: vec![],
            frame_rate: 30.0,
            duration_frames: 0,
        };
        let out = composite_frame(&timeline, 0, 2, 2, &provider(&[]));
        assert!(out.pixels.iter().all(|pixel| pixel[3] == 0.0));
    }

    #[test]
    fn missing_provider_frame_is_skipped() {
        let timeline = Timeline {
            tracks: vec![track("v1", vec![clip("a", "absent", 0, 30, 0)])],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let out = composite_frame(&timeline, 10, 2, 2, &provider(&[]));
        assert!(out.pixels.iter().all(|pixel| pixel[3] == 0.0));
    }
}
