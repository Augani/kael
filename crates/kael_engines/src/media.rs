//! Media and video workload engine.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::playback::Timebase;
use crate::timecode::Timecode;

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
    /// Layer opacity in `0..=1` applied when compositing (defaults to fully opaque).
    #[serde(default = "default_clip_opacity")]
    pub opacity: f32,
    /// How this clip composites over lower tracks (defaults to source-over).
    #[serde(default)]
    pub blend_mode: ClipBlendMode,
}

fn default_clip_opacity() -> f32 {
    1.0
}

/// How a clip composites over the tracks beneath it (serializable mirror of the
/// reference compositor's blend modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ClipBlendMode {
    /// Source-over (the default).
    #[default]
    Normal,
    /// Multiply (darken).
    Multiply,
    /// Screen (lighten).
    Screen,
    /// Clamped additive.
    Add,
    /// Per-channel minimum.
    Darken,
    /// Per-channel maximum.
    Lighten,
    /// Contrast (multiply/screen keyed on the backdrop).
    Overlay,
    /// Contrast keyed on the source.
    HardLight,
    /// Gentle dodge/burn.
    SoftLight,
    /// Brighten toward the source.
    ColorDodge,
    /// Darken toward the source.
    ColorBurn,
    /// Absolute difference.
    Difference,
    /// Lower-contrast difference.
    Exclusion,
}

impl ClipBlendMode {
    /// Map to the reference compositor's blend mode.
    pub fn to_render_graph(self) -> kael_render_graph::reference::BlendMode {
        use kael_render_graph::reference::BlendMode;
        match self {
            Self::Normal => BlendMode::Normal,
            Self::Multiply => BlendMode::Multiply,
            Self::Screen => BlendMode::Screen,
            Self::Add => BlendMode::Add,
            Self::Darken => BlendMode::Darken,
            Self::Lighten => BlendMode::Lighten,
            Self::Overlay => BlendMode::Overlay,
            Self::HardLight => BlendMode::HardLight,
            Self::SoftLight => BlendMode::SoftLight,
            Self::ColorDodge => BlendMode::ColorDodge,
            Self::ColorBurn => BlendMode::ColorBurn,
            Self::Difference => BlendMode::Difference,
            Self::Exclusion => BlendMode::Exclusion,
        }
    }
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
    /// Whether the track contributes to the composite/mix (defaults to enabled). A
    /// disabled track is hidden (video) or muted (audio).
    #[serde(default = "default_track_enabled")]
    pub enabled: bool,
}

fn default_track_enabled() -> bool {
    true
}

/// An error from a timeline edit operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineEditError {
    /// No clip with the given id exists on the track.
    ClipNotFound,
    /// The edit would produce a zero or negative clip duration.
    InvalidDuration,
    /// The edit would move a source point or track position before zero.
    OutOfBounds,
    /// The operation requires two adjacent clips that were not found.
    NotAdjacent,
    /// The split position is not strictly inside the clip.
    InvalidSplit,
}

impl std::fmt::Display for TimelineEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ClipNotFound => "clip not found",
            Self::InvalidDuration => "edit would produce a non-positive clip duration",
            Self::OutOfBounds => "edit would move past the start of the source or track",
            Self::NotAdjacent => "operation requires two adjacent clips",
            Self::InvalidSplit => "split position is not inside the clip",
        };
        f.write_str(message)
    }
}

impl std::error::Error for TimelineEditError {}

fn offset_by(value: u64, delta: i64) -> Result<u64, TimelineEditError> {
    let result = value as i128 + delta as i128;
    if result < 0 || result > u64::MAX as i128 {
        Err(TimelineEditError::OutOfBounds)
    } else {
        Ok(result as u64)
    }
}

fn subtract_track_range(clip: &TimelineClip, start: u64, end: u64) -> Vec<TimelineClip> {
    let clip_start = clip.track_offset;
    let clip_end = clip.track_end();
    if clip_end <= start || clip_start >= end {
        return vec![clip.clone()];
    }
    let has_left = clip_start < start;
    let mut pieces = Vec::new();
    if has_left {
        let mut left = clip.clone();
        left.end_frame = clip.start_frame + (start - clip_start);
        pieces.push(left);
    }
    if clip_end > end {
        let mut right = clip.clone();
        right.start_frame = clip.start_frame + (end - clip_start);
        right.track_offset = end;
        if has_left {
            right.id = format!("{}-r", clip.id);
        }
        pieces.push(right);
    }
    pieces
}

impl TimelineClip {
    /// The clip's duration in frames.
    pub fn duration(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    /// The exclusive end of the clip on the track.
    pub fn track_end(&self) -> u64 {
        self.track_offset + self.duration()
    }

    /// Whether the clip is visible at the given track (timeline) frame.
    pub fn contains_track_frame(&self, frame: u64) -> bool {
        frame >= self.track_offset && frame < self.track_end()
    }

    /// The source-media frame this clip displays at the given track frame, or
    /// `None` when the clip is not visible there. This is the exact frame a
    /// decoder must produce for frame-accurate playback.
    pub fn source_frame_at(&self, frame: u64) -> Option<u64> {
        if self.contains_track_frame(frame) {
            Some(self.start_frame + (frame - self.track_offset))
        } else {
            None
        }
    }
}

/// The decode request for one track at a given timeline frame: which source
/// frame of which clip must be produced.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackFrameRequest {
    /// The track the visible clip lives on.
    pub track_id: String,
    /// The clip that is visible at the requested frame.
    pub clip_id: String,
    /// The source-media path to decode from.
    pub source: String,
    /// The frame index within the source media to present.
    pub source_frame: u64,
    /// The clip's layer opacity in `0..=1`.
    pub opacity: f32,
    /// The clip's blend mode over lower tracks.
    pub blend_mode: ClipBlendMode,
}

impl TimelineTrack {
    fn clip_index(&self, id: &str) -> Result<usize, TimelineEditError> {
        self.clips
            .iter()
            .position(|clip| clip.id == id)
            .ok_or(TimelineEditError::ClipNotFound)
    }

    /// Trim the clip's in-point (left edge) by `delta`, keeping its out-point
    /// fixed on the track. Positive `delta` shortens the clip.
    pub fn trim_in(&mut self, id: &str, delta: i64) -> Result<(), TimelineEditError> {
        let index = self.clip_index(id)?;
        let clip = &self.clips[index];
        let new_start = offset_by(clip.start_frame, delta)?;
        let new_offset = offset_by(clip.track_offset, delta)?;
        if new_start >= clip.end_frame {
            return Err(TimelineEditError::InvalidDuration);
        }
        let clip = &mut self.clips[index];
        clip.start_frame = new_start;
        clip.track_offset = new_offset;
        Ok(())
    }

    /// Trim the clip's out-point (right edge) by `delta`. Positive `delta`
    /// lengthens the clip.
    pub fn trim_out(&mut self, id: &str, delta: i64) -> Result<(), TimelineEditError> {
        let index = self.clip_index(id)?;
        let new_end = offset_by(self.clips[index].end_frame, delta)?;
        if new_end <= self.clips[index].start_frame {
            return Err(TimelineEditError::InvalidDuration);
        }
        self.clips[index].end_frame = new_end;
        Ok(())
    }

    /// Trim the out-point by `delta` and ripple all later clips by the same
    /// amount, closing or opening the gap.
    pub fn ripple_trim_out(&mut self, id: &str, delta: i64) -> Result<(), TimelineEditError> {
        let index = self.clip_index(id)?;
        let pivot = self.clips[index].track_offset;
        self.trim_out(id, delta)?;
        for (i, clip) in self.clips.iter_mut().enumerate() {
            if i != index && clip.track_offset > pivot {
                clip.track_offset = offset_by(clip.track_offset, delta)?;
            }
        }
        Ok(())
    }

    /// Remove a clip and shift all later clips left to close the gap.
    pub fn ripple_delete(&mut self, id: &str) -> Result<TimelineClip, TimelineEditError> {
        let index = self.clip_index(id)?;
        let removed = self.clips.remove(index);
        let shift = removed.duration() as i64;
        for clip in self.clips.iter_mut() {
            if clip.track_offset > removed.track_offset {
                clip.track_offset = offset_by(clip.track_offset, -shift)?;
            }
        }
        Ok(removed)
    }

    /// Remove the clip with the given id, leaving a gap where it was — the non-rippling
    /// "lift" edit. Later clips keep their positions (contrast [`TimelineTrack::ripple_delete`],
    /// which closes the gap). Returns the removed clip.
    pub fn lift(&mut self, id: &str) -> Result<TimelineClip, TimelineEditError> {
        let index = self.clip_index(id)?;
        Ok(self.clips.remove(index))
    }

    /// Slip the clip's source window by `delta` (both in and out), leaving its
    /// track position and duration unchanged.
    pub fn slip(&mut self, id: &str, delta: i64) -> Result<(), TimelineEditError> {
        let index = self.clip_index(id)?;
        let new_start = offset_by(self.clips[index].start_frame, delta)?;
        let new_end = offset_by(self.clips[index].end_frame, delta)?;
        let clip = &mut self.clips[index];
        clip.start_frame = new_start;
        clip.end_frame = new_end;
        Ok(())
    }

    /// Roll the edit point between `left_id` and its immediately following,
    /// adjacent clip by `delta`, keeping the combined span fixed.
    pub fn roll(&mut self, left_id: &str, delta: i64) -> Result<(), TimelineEditError> {
        let left_index = self.clip_index(left_id)?;
        let boundary = self.clips[left_index].track_end();
        let right_index = self
            .clips
            .iter()
            .position(|clip| clip.track_offset == boundary)
            .ok_or(TimelineEditError::NotAdjacent)?;

        let new_left_end = offset_by(self.clips[left_index].end_frame, delta)?;
        if new_left_end <= self.clips[left_index].start_frame {
            return Err(TimelineEditError::InvalidDuration);
        }
        let new_right_start = offset_by(self.clips[right_index].start_frame, delta)?;
        let new_right_offset = offset_by(self.clips[right_index].track_offset, delta)?;
        if new_right_start >= self.clips[right_index].end_frame {
            return Err(TimelineEditError::InvalidDuration);
        }

        self.clips[left_index].end_frame = new_left_end;
        self.clips[right_index].start_frame = new_right_start;
        self.clips[right_index].track_offset = new_right_offset;
        Ok(())
    }

    /// Slide the clip by `delta` track frames, absorbing the move into the
    /// adjacent previous and next clips (its own content and duration unchanged).
    /// Requires adjacent neighbors on both sides.
    pub fn slide(&mut self, id: &str, delta: i64) -> Result<(), TimelineEditError> {
        let index = self.clip_index(id)?;
        let start = self.clips[index].track_offset;
        let end = self.clips[index].track_end();
        let prev_index = self
            .clips
            .iter()
            .position(|clip| clip.track_end() == start)
            .ok_or(TimelineEditError::NotAdjacent)?;
        let next_index = self
            .clips
            .iter()
            .position(|clip| clip.track_offset == end)
            .ok_or(TimelineEditError::NotAdjacent)?;

        let new_offset = offset_by(self.clips[index].track_offset, delta)?;
        let new_prev_end = offset_by(self.clips[prev_index].end_frame, delta)?;
        let new_next_start = offset_by(self.clips[next_index].start_frame, delta)?;
        let new_next_offset = offset_by(self.clips[next_index].track_offset, delta)?;
        if new_prev_end <= self.clips[prev_index].start_frame
            || new_next_start >= self.clips[next_index].end_frame
        {
            return Err(TimelineEditError::InvalidDuration);
        }

        self.clips[index].track_offset = new_offset;
        self.clips[prev_index].end_frame = new_prev_end;
        self.clips[next_index].start_frame = new_next_start;
        self.clips[next_index].track_offset = new_next_offset;
        Ok(())
    }

    /// Split the clip at `at_track_frame`, returning the id of the new right clip.
    pub fn split(&mut self, id: &str, at_track_frame: u64) -> Result<String, TimelineEditError> {
        let index = self.clip_index(id)?;
        let clip = self.clips[index].clone();
        if at_track_frame <= clip.track_offset || at_track_frame >= clip.track_end() {
            return Err(TimelineEditError::InvalidSplit);
        }
        let split_source = clip.start_frame + (at_track_frame - clip.track_offset);
        let new_id = format!("{}-split", clip.id);

        self.clips[index].end_frame = split_source;
        self.clips.insert(
            index + 1,
            TimelineClip {
                id: new_id.clone(),
                source: clip.source,
                start_frame: split_source,
                end_frame: clip.end_frame,
                track_offset: at_track_frame,
                opacity: clip.opacity,
                blend_mode: clip.blend_mode,
            },
        );
        Ok(new_id)
    }

    /// Insert `clip` at its `track_offset`, rippling clips at or after that
    /// position right by the clip's duration (no overwrite).
    pub fn insert_clip(&mut self, clip: TimelineClip) {
        let at = clip.track_offset;
        let shift = clip.duration();
        for existing in &mut self.clips {
            if existing.track_offset >= at {
                existing.track_offset += shift;
            }
        }
        self.clips.push(clip);
    }

    /// Place `clip` at its track span, overwriting overlapping content: clips it
    /// fully covers are removed, partially-overlapped clips are trimmed, and a
    /// clip it lands inside is split around it.
    pub fn overwrite_clip(&mut self, clip: TimelineClip) {
        let (start, end) = (clip.track_offset, clip.track_end());
        let mut next = Vec::new();
        for existing in std::mem::take(&mut self.clips) {
            next.extend(subtract_track_range(&existing, start, end));
        }
        next.push(clip);
        self.clips = next;
    }

    /// Empty ranges between consecutive clips, in track order.
    pub fn gaps(&self) -> Vec<(u64, u64)> {
        let mut clips: Vec<&TimelineClip> = self.clips.iter().collect();
        clips.sort_by_key(|clip| clip.track_offset);
        let mut gaps = Vec::new();
        let mut cursor = 0u64;
        for clip in clips {
            if clip.track_offset > cursor {
                gaps.push((cursor, clip.track_offset));
            }
            cursor = cursor.max(clip.track_end());
        }
        gaps
    }

    /// Whether any two clips overlap on the track.
    pub fn has_overlap(&self) -> bool {
        let mut clips: Vec<&TimelineClip> = self.clips.iter().collect();
        clips.sort_by_key(|clip| clip.track_offset);
        clips
            .windows(2)
            .any(|pair| pair[1].track_offset < pair[0].track_end())
    }
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
                let Some(duration) = c.end_frame.checked_sub(c.start_frame) else {
                    return false;
                };
                let Some(clip_end) = c.track_offset.checked_add(duration) else {
                    return false;
                };
                frame >= c.track_offset && frame < clip_end
            })
            .collect()
    }

    /// Recompute `duration_frames` from the furthest clip end across all tracks.
    pub fn recompute_duration(&mut self) {
        self.duration_frames = self
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.track_end())
            .max()
            .unwrap_or(0);
    }

    /// The exact timebase (rational fps) of this timeline, or `None` if the frame
    /// rate is not a usable value.
    pub fn timebase(&self) -> Option<Timebase> {
        Timebase::from_fps(self.frame_rate)
    }

    /// The SMPTE (non-drop) timecode for timeline `frame` at this timeline's frame rate
    /// (rounded to the nearest integer), or `None` if the frame rate is not usable.
    pub fn timecode_at(&self, frame: u64) -> Option<Timecode> {
        if self.frame_rate <= 0.0 || !self.frame_rate.is_finite() {
            return None;
        }
        Some(Timecode::from_frame(frame, self.frame_rate.round() as u32))
    }

    /// Resolve, for each track, the source frame that must be decoded to present
    /// timeline `frame` — the transport's per-tick decode schedule.
    ///
    /// Tracks with no clip at `frame` are omitted; tracks are returned in
    /// declaration order, and within a track the first clip covering the frame wins.
    pub fn frame_requests(&self, frame: u64) -> Vec<TrackFrameRequest> {
        let mut requests = Vec::new();
        for track in &self.tracks {
            if !track.enabled {
                continue;
            }
            if let Some((clip, source_frame)) = track
                .clips
                .iter()
                .find_map(|clip| clip.source_frame_at(frame).map(|sf| (clip, sf)))
            {
                requests.push(TrackFrameRequest {
                    track_id: track.id.clone(),
                    clip_id: clip.id.clone(),
                    source: clip.source.clone(),
                    source_frame,
                    opacity: clip.opacity,
                    blend_mode: clip.blend_mode,
                });
            }
        }
        requests
    }

    /// The set of frames an edit can snap to: clip edges (in and out) across every
    /// track plus the timeline origin, sorted ascending and de-duplicated.
    pub fn snap_points(&self) -> Vec<u64> {
        let mut points = vec![0u64];
        for track in &self.tracks {
            for clip in &track.clips {
                points.push(clip.track_offset);
                points.push(clip.track_end());
            }
        }
        points.sort_unstable();
        points.dedup();
        points
    }

    /// Snap `frame` to the nearest [`snap_point`](Timeline::snap_points) within
    /// `threshold` frames, or return `frame` unchanged when none is close enough.
    /// Ties prefer the earlier snap point.
    pub fn snap_frame(&self, frame: u64, threshold: u64) -> u64 {
        self.snap_frame_with(frame, threshold, &[])
    }

    /// Like [`snap_frame`](Timeline::snap_frame) but also considers `extra_points`
    /// (e.g. marker frames from a [`MarkerSet`](crate::markers::MarkerSet)) as snap
    /// targets alongside clip edges.
    pub fn snap_frame_with(&self, frame: u64, threshold: u64, extra_points: &[u64]) -> u64 {
        let mut best: Option<(u64, u64)> = None;
        let candidates = self
            .snap_points()
            .into_iter()
            .chain(extra_points.iter().copied());
        for point in candidates {
            let distance = frame.abs_diff(point);
            if distance <= threshold && best.is_none_or(|(_, closest)| distance < closest) {
                best = Some((point, distance));
            }
        }
        best.map(|(point, _)| point).unwrap_or(frame)
    }

    /// Ripple-insert `frames` of empty space at `at_frame` across every track: every
    /// clip that begins at or after `at_frame` shifts later by `frames`, keeping all
    /// tracks in sync. Recomputes the timeline duration.
    pub fn ripple_insert_gap(&mut self, at_frame: u64, frames: u64) {
        if frames == 0 {
            return;
        }
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.track_offset >= at_frame {
                    clip.track_offset += frames;
                }
            }
        }
        self.recompute_duration();
    }

    /// Ripple-close a `frames`-wide gap starting at `at_frame` across every track:
    /// every clip that begins at or after the end of the gap shifts earlier by
    /// `frames`. The caller is responsible for the span being an actual gap (no clip
    /// straddles it). Recomputes the timeline duration.
    pub fn ripple_close_gap(&mut self, at_frame: u64, frames: u64) {
        if frames == 0 {
            return;
        }
        let gap_end = at_frame + frames;
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.track_offset >= gap_end {
                    clip.track_offset -= frames;
                }
            }
        }
        self.recompute_duration();
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
            opacity: 1.0,
            blend_mode: ClipBlendMode::Normal,
        }
    }

    fn sample_track(id: &str, track_type: TrackType, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: id.to_string(),
            name: format!("Track {id}"),
            track_type,
            clips,
            enabled: true,
        }
    }

    #[test]
    fn clip_source_frame_maps_track_to_source() {
        let clip = sample_clip("c", 100, 110, 50);
        assert_eq!(clip.duration(), 10);
        assert_eq!(clip.track_end(), 60);
        assert!(!clip.contains_track_frame(49));
        assert!(clip.contains_track_frame(50));
        assert!(clip.contains_track_frame(59));
        assert!(!clip.contains_track_frame(60));
        assert_eq!(clip.source_frame_at(50), Some(100));
        assert_eq!(clip.source_frame_at(55), Some(105));
        assert_eq!(clip.source_frame_at(59), Some(109));
        assert_eq!(clip.source_frame_at(60), None);
        assert_eq!(clip.source_frame_at(0), None);
    }

    #[test]
    fn timeline_frame_requests_resolve_active_source_frames_per_track() {
        let tl = Timeline {
            tracks: vec![
                sample_track(
                    "v1",
                    TrackType::Video,
                    vec![sample_clip("a", 0, 30, 0), sample_clip("b", 200, 260, 30)],
                ),
                sample_track("v2", TrackType::Video, vec![sample_clip("c", 500, 560, 40)]),
            ],
            frame_rate: 30.0,
            duration_frames: 100,
        };

        let early = tl.frame_requests(10);
        assert_eq!(early.len(), 1);
        assert_eq!(early[0].track_id, "v1");
        assert_eq!(early[0].clip_id, "a");
        assert_eq!(early[0].source_frame, 10);

        let mid = tl.frame_requests(45);
        assert_eq!(mid.len(), 2);
        assert_eq!((mid[0].clip_id.as_str(), mid[0].source_frame), ("b", 215));
        assert_eq!(
            (mid[1].track_id.as_str(), mid[1].clip_id.as_str(), mid[1].source_frame),
            ("v2", "c", 505)
        );

        assert!(tl.frame_requests(100).is_empty());
    }

    #[test]
    fn disabled_track_is_excluded_from_frame_requests() {
        let mut tl = Timeline {
            tracks: vec![
                sample_track("v1", TrackType::Video, vec![sample_clip("a", 0, 60, 0)]),
                sample_track("v2", TrackType::Video, vec![sample_clip("b", 0, 60, 0)]),
            ],
            frame_rate: 30.0,
            duration_frames: 60,
        };

        assert_eq!(tl.frame_requests(10).len(), 2);

        tl.tracks[0].enabled = false;
        let active = tl.frame_requests(10);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].track_id, "v2");
    }

    #[test]
    fn timeline_timebase_snaps_to_exact_rational() {
        let tl = Timeline {
            tracks: vec![],
            frame_rate: 29.97,
            duration_frames: 0,
        };
        assert_eq!(tl.timebase(), Some(Timebase::NTSC));
        let bad = Timeline {
            tracks: vec![],
            frame_rate: 0.0,
            duration_frames: 0,
        };
        assert_eq!(bad.timebase(), None);
    }

    #[test]
    fn timeline_timecode_at_uses_rounded_fps() {
        let tl = Timeline {
            tracks: vec![],
            frame_rate: 30.0,
            duration_frames: 0,
        };
        assert_eq!(tl.timecode_at(90).unwrap().to_string(), "00:00:03:00");
        // 29.97 rounds to 30 fps for the displayed timecode.
        let ntsc = Timeline {
            tracks: vec![],
            frame_rate: 29.97,
            duration_frames: 0,
        };
        assert_eq!(ntsc.timecode_at(30).unwrap().to_string(), "00:00:01:00");
        let bad = Timeline {
            tracks: vec![],
            frame_rate: 0.0,
            duration_frames: 0,
        };
        assert!(bad.timecode_at(0).is_none());
    }

    #[test]
    fn snap_points_are_clip_edges_plus_origin() {
        let tl = Timeline {
            tracks: vec![
                sample_track("v1", TrackType::Video, vec![sample_clip("a", 0, 30, 10)]),
                sample_track("v2", TrackType::Video, vec![sample_clip("b", 0, 20, 50)]),
            ],
            frame_rate: 30.0,
            duration_frames: 70,
        };
        // origin 0, clip a [10,40), clip b [50,70) -> 0,10,40,50,70.
        assert_eq!(tl.snap_points(), vec![0, 10, 40, 50, 70]);
    }

    #[test]
    fn snap_frame_pulls_to_nearest_edge_within_threshold() {
        let tl = Timeline {
            tracks: vec![sample_track(
                "v1",
                TrackType::Video,
                vec![sample_clip("a", 0, 30, 100)],
            )],
            frame_rate: 30.0,
            duration_frames: 130,
        };
        // Near the in-edge 100.
        assert_eq!(tl.snap_frame(103, 5), 100);
        // Near the out-edge 130.
        assert_eq!(tl.snap_frame(128, 5), 130);
        // Outside the threshold -> unchanged.
        assert_eq!(tl.snap_frame(115, 5), 115);
        // Exactly on a point stays put.
        assert_eq!(tl.snap_frame(100, 5), 100);
    }

    #[test]
    fn snap_frame_with_considers_marker_frames() {
        let tl = Timeline {
            tracks: vec![sample_track(
                "v1",
                TrackType::Video,
                vec![sample_clip("a", 0, 30, 0)],
            )],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        // No clip edge near 200, but a marker is.
        assert_eq!(tl.snap_frame(202, 5), 202);
        assert_eq!(tl.snap_frame_with(202, 5, &[200]), 200);
        // Clip edge still wins when closer than the marker.
        assert_eq!(tl.snap_frame_with(31, 5, &[36]), 30);
    }

    #[test]
    fn ripple_insert_gap_shifts_all_tracks_in_sync() {
        let mut tl = Timeline {
            tracks: vec![
                sample_track("v1", TrackType::Video, vec![sample_clip("a", 0, 20, 50)]),
                sample_track("v2", TrackType::Video, vec![sample_clip("b", 0, 20, 80)]),
            ],
            frame_rate: 30.0,
            duration_frames: 100,
        };
        tl.ripple_insert_gap(60, 10);
        // Clip a (offset 50, before 60) stays; clip b (offset 80, after) shifts +10.
        assert_eq!(tl.tracks[0].clips[0].track_offset, 50);
        assert_eq!(tl.tracks[1].clips[0].track_offset, 90);
        assert_eq!(tl.duration_frames, 110);
    }

    #[test]
    fn ripple_close_gap_pulls_later_clips_earlier() {
        let mut tl = Timeline {
            tracks: vec![sample_track(
                "v1",
                TrackType::Video,
                vec![sample_clip("a", 0, 20, 0), sample_clip("b", 0, 20, 40)],
            )],
            frame_rate: 30.0,
            duration_frames: 60,
        };
        // Gap [20,40) is empty; close it.
        tl.ripple_close_gap(20, 20);
        assert_eq!(tl.tracks[0].clips[0].track_offset, 0);
        assert_eq!(tl.tracks[0].clips[1].track_offset, 20);
        assert_eq!(tl.duration_frames, 40);
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
    fn clips_at_frame_rejects_invalid_clip_ranges() {
        let invalid = sample_clip("invalid", 30, 10, u64::MAX - 2);
        let track = sample_track("v1", TrackType::Video, vec![invalid]);
        let tl = Timeline {
            tracks: vec![track],
            frame_rate: 30.0,
            duration_frames: 60,
        };

        assert!(tl.clips_at_frame(u64::MAX - 1).is_empty());
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
    fn zero_duration_clip_is_valid() {
        let clip = sample_clip("zero", 10, 10, 0);
        let track = sample_track("v1", TrackType::Video, vec![clip]);
        let tl = Timeline {
            tracks: vec![track],
            frame_rate: 30.0,
            duration_frames: 60,
        };
        assert!(tl.clips_at_frame(0).is_empty());
        assert!(tl.clips_at_frame(10).is_empty());
    }

    #[test]
    fn max_frame_arithmetic_does_not_overflow() {
        let clip = sample_clip("big", u64::MAX - 10, u64::MAX, u64::MAX - 10);
        let track = sample_track("v1", TrackType::Video, vec![clip]);
        let tl = Timeline {
            tracks: vec![track],
            frame_rate: 30.0,
            duration_frames: u64::MAX,
        };
        let _ = tl.clips_at_frame(u64::MAX);
        let _ = tl.clips_at_frame(0);
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

    fn edit_track(clips: Vec<TimelineClip>) -> TimelineTrack {
        sample_track("v1", TrackType::Video, clips)
    }

    #[test]
    fn clip_duration_and_track_end() {
        let clip = sample_clip("a", 10, 40, 100);
        assert_eq!(clip.duration(), 30);
        assert_eq!(clip.track_end(), 130);
    }

    #[test]
    fn trim_in_shortens_and_keeps_out_fixed() {
        let mut track = edit_track(vec![sample_clip("a", 0, 100, 0)]);
        track.trim_in("a", 20).unwrap();
        let clip = &track.clips[0];
        assert_eq!(clip.start_frame, 20);
        assert_eq!(clip.track_offset, 20);
        assert_eq!(clip.track_end(), 100);
        assert_eq!(clip.duration(), 80);
    }

    #[test]
    fn trim_out_changes_duration_with_bounds() {
        let mut track = edit_track(vec![sample_clip("a", 0, 100, 0)]);
        track.trim_out("a", -30).unwrap();
        assert_eq!(track.clips[0].duration(), 70);
        assert_eq!(
            track.trim_out("a", -70),
            Err(TimelineEditError::InvalidDuration)
        );
        assert_eq!(
            track.trim_out("a", -1000),
            Err(TimelineEditError::OutOfBounds)
        );
        assert_eq!(
            track.trim_out("missing", 1),
            Err(TimelineEditError::ClipNotFound)
        );
    }

    #[test]
    fn ripple_trim_out_shifts_later_clips() {
        let mut track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 50),
        ]);
        track.ripple_trim_out("a", -10).unwrap();
        assert_eq!(track.clips[0].duration(), 40);
        assert_eq!(track.clips[1].track_offset, 40);
        assert!(track.gaps().is_empty());
    }

    #[test]
    fn ripple_delete_closes_the_gap() {
        let mut track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 50),
            sample_clip("c", 0, 50, 100),
        ]);
        track.ripple_delete("b").unwrap();
        assert_eq!(track.clips.len(), 2);
        let c = track.clips.iter().find(|clip| clip.id == "c").unwrap();
        assert_eq!(c.track_offset, 50);
        assert!(!track.has_overlap());
    }

    #[test]
    fn lift_leaves_a_gap() {
        let mut track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 50),
            sample_clip("c", 0, 50, 100),
        ]);
        let lifted = track.lift("b").unwrap();
        assert_eq!(lifted.id, "b");
        assert_eq!(track.clips.len(), 2);
        // Unlike ripple_delete, the later clip stays put and a gap remains.
        let c = track.clips.iter().find(|clip| clip.id == "c").unwrap();
        assert_eq!(c.track_offset, 100);
        assert_eq!(track.gaps(), vec![(50, 100)]);
        assert!(track.lift("missing").is_err());
    }

    #[test]
    fn slip_changes_source_not_position() {
        let mut track = edit_track(vec![sample_clip("a", 10, 60, 100)]);
        track.slip("a", 5).unwrap();
        let clip = &track.clips[0];
        assert_eq!((clip.start_frame, clip.end_frame), (15, 65));
        assert_eq!(clip.track_offset, 100);
        assert_eq!(clip.duration(), 50);
        assert_eq!(track.slip("a", -100), Err(TimelineEditError::OutOfBounds));
    }

    #[test]
    fn roll_moves_boundary_keeping_span() {
        let mut track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 50),
        ]);
        let span = track.clips[0].duration() + track.clips[1].duration();
        track.roll("a", 10).unwrap();
        assert_eq!(track.clips[0].track_end(), 60);
        let b = track.clips.iter().find(|clip| clip.id == "b").unwrap();
        assert_eq!(b.track_offset, 60);
        assert_eq!(b.start_frame, 10);
        assert_eq!(track.clips[0].duration() + b.duration(), span);
        assert!(!track.has_overlap());
    }

    #[test]
    fn slide_moves_clip_absorbing_into_neighbors() {
        let mut track = edit_track(vec![
            sample_clip("prev", 0, 50, 0),
            sample_clip("mid", 0, 50, 50),
            sample_clip("next", 0, 50, 100),
        ]);
        track.slide("mid", 10).unwrap();
        let prev = track.clips.iter().find(|clip| clip.id == "prev").unwrap();
        let mid = track.clips.iter().find(|clip| clip.id == "mid").unwrap();
        let next = track.clips.iter().find(|clip| clip.id == "next").unwrap();
        assert_eq!(mid.track_offset, 60);
        assert_eq!(mid.duration(), 50);
        assert_eq!((prev.end_frame, prev.track_end()), (60, 60));
        assert_eq!((next.start_frame, next.track_offset), (10, 110));
        assert!(!track.has_overlap());
        assert!(track.gaps().is_empty());
        assert_eq!(
            edit_track(vec![sample_clip("solo", 0, 50, 0)]).slide("solo", 10),
            Err(TimelineEditError::NotAdjacent)
        );
    }

    #[test]
    fn split_creates_two_adjacent_clips() {
        let mut track = edit_track(vec![sample_clip("a", 100, 200, 0)]);
        let new_id = track.split("a", 30).unwrap();
        assert_eq!(track.clips.len(), 2);
        assert_eq!(track.clips[0].end_frame, 130);
        assert_eq!(track.clips[0].track_end(), 30);
        let right = track.clips.iter().find(|clip| clip.id == new_id).unwrap();
        assert_eq!(
            (right.start_frame, right.end_frame, right.track_offset),
            (130, 200, 30)
        );
        assert_eq!(track.split("a", 0), Err(TimelineEditError::InvalidSplit));
        assert!(!track.has_overlap());
    }

    #[test]
    fn gaps_and_overlap_detection() {
        let track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 80),
        ]);
        assert_eq!(track.gaps(), vec![(50, 80)]);
        assert!(!track.has_overlap());

        let overlapping = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 30),
        ]);
        assert!(overlapping.has_overlap());
    }

    #[test]
    fn insert_clip_ripples_later_clips() {
        let mut track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 50),
        ]);
        track.insert_clip(sample_clip("c", 0, 20, 50));
        let b = track.clips.iter().find(|clip| clip.id == "b").unwrap();
        assert_eq!(b.track_offset, 70);
        let a = track.clips.iter().find(|clip| clip.id == "a").unwrap();
        assert_eq!(a.track_offset, 0);
        assert!(!track.has_overlap());
        assert!(track.gaps().is_empty());
    }

    #[test]
    fn overwrite_clip_splits_covered_clip() {
        let mut track = edit_track(vec![sample_clip("a", 0, 100, 0)]);
        track.overwrite_clip(sample_clip("c", 200, 220, 20));
        assert!(!track.has_overlap());
        let left = track.clips.iter().find(|clip| clip.id == "a").unwrap();
        assert_eq!((left.track_offset, left.track_end()), (0, 20));
        let right = track.clips.iter().find(|clip| clip.id == "a-r").unwrap();
        assert_eq!(
            (right.track_offset, right.track_end(), right.start_frame),
            (40, 100, 40)
        );
        assert!(track.clips.iter().any(|clip| clip.id == "c"));
    }

    #[test]
    fn overwrite_clip_trims_partial_overlap_and_drops_covered() {
        let mut track = edit_track(vec![
            sample_clip("a", 0, 50, 0),
            sample_clip("b", 0, 50, 50),
        ]);
        // covers all of `a` from 40 and all of `b` up to 60.
        track.overwrite_clip(sample_clip("c", 0, 20, 40));
        assert!(!track.has_overlap());
        let a = track.clips.iter().find(|clip| clip.id == "a").unwrap();
        assert_eq!(a.track_end(), 40);
        let b = track.clips.iter().find(|clip| clip.id == "b").unwrap();
        assert_eq!(b.track_offset, 60);
    }
}
