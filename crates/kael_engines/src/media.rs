//! Media and video workload engine.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Metadata extracted from probing a media file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaProbe {
    /// File path.
    pub path: String,
    /// Container format (e.g. "mp4", "mkv").
    pub format: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Video codec identifier.
    pub video_codec: Option<String>,
    /// Audio codec identifier.
    pub audio_codec: Option<String>,
    /// Video width in pixels.
    pub width: Option<u32>,
    /// Video height in pixels.
    pub height: Option<u32>,
    /// Frames per second.
    pub frame_rate: Option<f64>,
    /// Bitrate in bits per second.
    pub bitrate: Option<u64>,
}

/// The kind of content a track carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackType {
    /// Video frames.
    Video,
    /// Audio samples.
    Audio,
    /// Subtitle text.
    Subtitle,
    /// Visual/audio effect.
    Effect,
}

/// A clip placed on a timeline track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineClip {
    /// Unique clip identifier.
    pub id: String,
    /// Source media path.
    pub source: String,
    /// First frame (inclusive) in source media.
    pub start_frame: u64,
    /// Last frame (exclusive) in source media.
    pub end_frame: u64,
    /// Offset in track frames where the clip begins.
    pub track_offset: u64,
}

/// A single track within a timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineTrack {
    /// Unique track identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Content type.
    pub track_type: TrackType,
    /// Ordered list of clips.
    pub clips: Vec<TimelineClip>,
}

/// A multi-track editing timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// All tracks in this timeline.
    pub tracks: Vec<TimelineTrack>,
    /// Frames per second for this timeline.
    pub frame_rate: f64,
    /// Total duration measured in frames.
    pub duration_frames: u64,
}

impl Timeline {
    /// Add a track to the timeline.
    pub fn add_track(&mut self, track: TimelineTrack) {
        self.tracks.push(track);
    }

    /// Remove a track by id. Returns the removed track if found.
    pub fn remove_track(&mut self, id: &str) -> Option<TimelineTrack> {
        if let Some(pos) = self.tracks.iter().position(|t| t.id == id) {
            Some(self.tracks.remove(pos))
        } else {
            None
        }
    }

    /// Get a reference to a track by id.
    pub fn get_track(&self, id: &str) -> Option<&TimelineTrack> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Total duration in milliseconds derived from frame count and rate.
    pub fn total_duration_ms(&self) -> u64 {
        if self.frame_rate <= 0.0 {
            return 0;
        }
        ((self.duration_frames as f64 / self.frame_rate) * 1000.0) as u64
    }

    /// Return all clips that overlap the given frame position.
    pub fn clips_at_frame(&self, frame: u64) -> Vec<&TimelineClip> {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .filter(|c| {
                let clip_end = c.track_offset + (c.end_frame - c.start_frame);
                frame >= c.track_offset && frame < clip_end
            })
            .collect()
    }
}

/// A request to generate a thumbnail from a media source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailRequest {
    /// Source media path.
    pub source: String,
    /// Position in the source to capture.
    pub timestamp_ms: u64,
    /// Desired thumbnail width.
    pub width: u32,
    /// Desired thumbnail height.
    pub height: u32,
}

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cached entries.
    pub entries: usize,
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
}

/// In-memory cache for generated thumbnails.
#[derive(Debug, Default)]
pub struct ThumbnailCache {
    store: HashMap<String, Vec<u8>>,
    hits: u64,
    misses: u64,
}

impl ThumbnailCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a thumbnail for the given request. Returns a cache key.
    pub fn request(&mut self, req: &ThumbnailRequest, data: Vec<u8>) -> String {
        let key = format!(
            "{}:{}:{}x{}",
            req.source, req.timestamp_ms, req.width, req.height
        );
        self.store.insert(key.clone(), data);
        key
    }

    /// Retrieve cached thumbnail bytes by key.
    pub fn get(&mut self, key: &str) -> Option<&Vec<u8>> {
        if self.store.contains_key(key) {
            self.hits += 1;
            self.store.get(key)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Remove a single cached thumbnail.
    pub fn invalidate(&mut self, key: &str) -> bool {
        self.store.remove(key).is_some()
    }

    /// Remove all cached thumbnails.
    pub fn clear(&mut self) {
        self.store.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Return current cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.store.len(),
            hits: self.hits,
            misses: self.misses,
        }
    }
}

/// Supported export container/codec formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// MPEG-4 container.
    Mp4,
    /// QuickTime container.
    Mov,
    /// Matroska container.
    Mkv,
    /// WAV audio.
    Wav,
    /// MP3 audio.
    Mp3,
    /// PNG image sequence.
    Png,
    /// Custom format string.
    Custom(String),
}

/// Configuration for exporting media.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    /// Target format.
    pub format: ExportFormat,
    /// Output file path.
    pub output_path: String,
    /// Output width override.
    pub width: Option<u32>,
    /// Output height override.
    pub height: Option<u32>,
    /// Output frame rate override.
    pub frame_rate: Option<f64>,
    /// Audio sample rate override.
    pub audio_sample_rate: Option<u32>,
}

impl ExportConfig {
    /// Validate the export configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.output_path.is_empty() {
            anyhow::bail!("output_path must not be empty");
        }
        if let Some(w) = self.width
            && w == 0
        {
            anyhow::bail!("width must be greater than zero");
        }
        if let Some(h) = self.height
            && h == 0
        {
            anyhow::bail!("height must be greater than zero");
        }
        if let Some(fr) = self.frame_rate
            && fr <= 0.0
        {
            anyhow::bail!("frame_rate must be positive");
        }
        if let Some(sr) = self.audio_sample_rate
            && sr == 0
        {
            anyhow::bail!("audio_sample_rate must be greater than zero");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_clip(id: &str, start: u64, end: u64, offset: u64) -> TimelineClip {
        TimelineClip {
            id: id.to_string(),
            source: "test.mp4".to_string(),
            start_frame: start,
            end_frame: end,
            track_offset: offset,
        }
    }

    fn sample_track(id: &str, track_type: TrackType, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: id.to_string(),
            name: format!("Track {id}"),
            track_type,
            clips,
        }
    }

    #[test]
    fn timeline_add_and_get_track() {
        let mut tl = Timeline {
            tracks: vec![],
            frame_rate: 30.0,
            duration_frames: 900,
        };
        tl.add_track(sample_track("v1", TrackType::Video, vec![]));
        assert_eq!(tl.tracks.len(), 1);
        assert!(tl.get_track("v1").is_some());
        assert!(tl.get_track("missing").is_none());
    }

    #[test]
    fn timeline_remove_track() {
        let mut tl = Timeline {
            tracks: vec![],
            frame_rate: 24.0,
            duration_frames: 480,
        };
        tl.add_track(sample_track("a1", TrackType::Audio, vec![]));
        assert!(tl.remove_track("a1").is_some());
        assert!(tl.remove_track("a1").is_none());
        assert!(tl.tracks.is_empty());
    }

    #[test]
    fn timeline_total_duration_ms() {
        let tl = Timeline {
            tracks: vec![],
            frame_rate: 30.0,
            duration_frames: 900,
        };
        assert_eq!(tl.total_duration_ms(), 30000);
    }

    #[test]
    fn timeline_total_duration_zero_rate() {
        let tl = Timeline {
            tracks: vec![],
            frame_rate: 0.0,
            duration_frames: 100,
        };
        assert_eq!(tl.total_duration_ms(), 0);
    }

    #[test]
    fn timeline_clips_at_frame() {
        let clip = sample_clip("c1", 0, 30, 10);
        let track = sample_track("v1", TrackType::Video, vec![clip]);
        let tl = Timeline {
            tracks: vec![track],
            frame_rate: 30.0,
            duration_frames: 60,
        };
        assert_eq!(tl.clips_at_frame(10).len(), 1);
        assert_eq!(tl.clips_at_frame(39).len(), 1);
        assert!(tl.clips_at_frame(40).is_empty());
        assert!(tl.clips_at_frame(9).is_empty());
    }

    #[test]
    fn thumbnail_cache_request_and_get() {
        let mut cache = ThumbnailCache::new();
        let req = ThumbnailRequest {
            source: "a.mp4".into(),
            timestamp_ms: 1000,
            width: 160,
            height: 90,
        };
        let key = cache.request(&req, vec![1, 2, 3]);
        assert_eq!(cache.get(&key).unwrap(), &vec![1, 2, 3]);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn thumbnail_cache_invalidate_and_clear() {
        let mut cache = ThumbnailCache::new();
        let req = ThumbnailRequest {
            source: "b.mp4".into(),
            timestamp_ms: 0,
            width: 64,
            height: 64,
        };
        let key = cache.request(&req, vec![9]);
        assert!(cache.invalidate(&key));
        assert!(!cache.invalidate(&key));
        let key2 = cache.request(&req, vec![8]);
        assert_eq!(cache.stats().entries, 1);
        cache.clear();
        assert_eq!(cache.stats().entries, 0);
        let _ = key2;
    }

    #[test]
    fn thumbnail_cache_stats() {
        let mut cache = ThumbnailCache::new();
        let req = ThumbnailRequest {
            source: "c.mp4".into(),
            timestamp_ms: 500,
            width: 32,
            height: 32,
        };
        let key = cache.request(&req, vec![0]);
        cache.get(&key);
        cache.get("miss");
        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn export_config_validate_ok() {
        let cfg = ExportConfig {
            format: ExportFormat::Mp4,
            output_path: "out.mp4".into(),
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            audio_sample_rate: Some(44100),
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn export_config_validate_empty_path() {
        let cfg = ExportConfig {
            format: ExportFormat::Wav,
            output_path: String::new(),
            width: None,
            height: None,
            frame_rate: None,
            audio_sample_rate: None,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn export_config_validate_zero_dimensions() {
        let cfg = ExportConfig {
            format: ExportFormat::Mp4,
            output_path: "x.mp4".into(),
            width: Some(0),
            height: None,
            frame_rate: None,
            audio_sample_rate: None,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn export_config_validate_negative_framerate() {
        let cfg = ExportConfig {
            format: ExportFormat::Mkv,
            output_path: "x.mkv".into(),
            width: None,
            height: None,
            frame_rate: Some(-1.0),
            audio_sample_rate: None,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn media_probe_serialization() {
        let probe = MediaProbe {
            path: "test.mp4".into(),
            format: "mp4".into(),
            duration_ms: 5000,
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(29.97),
            bitrate: Some(8_000_000),
        };
        let json = serde_json::to_string(&probe).unwrap();
        let deser: MediaProbe = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.path, "test.mp4");
        assert_eq!(deser.duration_ms, 5000);
    }
}
