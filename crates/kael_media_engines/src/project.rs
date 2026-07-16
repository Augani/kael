//! Versioned, relinkable project document model.
//!
//! Wraps a [`Timeline`](crate::media::Timeline) in a format-versioned envelope so
//! projects can evolve safely across releases: older documents are migrated to
//! the current format on load, newer-than-supported documents are rejected
//! rather than misread, and media can be relinked when sources move.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::markers::MarkerSet;
use crate::media::Timeline;

/// The current on-disk project format version.
pub const PROJECT_FORMAT_VERSION: u32 = 2;

/// An error reading or migrating a project document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectError {
    /// The JSON could not be parsed or serialized.
    Parse(String),
    /// The document's format version is newer than this build supports.
    UnsupportedVersion(u64),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "could not parse project: {message}"),
            Self::UnsupportedVersion(version) => write!(
                f,
                "unsupported project format version {version}; this build supports up to {PROJECT_FORMAT_VERSION}"
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

fn default_format_version() -> u32 {
    1
}

/// A complete editing project: a versioned wrapper around a [`Timeline`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// On-disk format version.
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    /// Project display name.
    #[serde(default)]
    pub name: String,
    /// The editing timeline.
    pub timeline: Timeline,
    /// Sequence markers (cue / chapter points).
    #[serde(default)]
    pub markers: MarkerSet,
    /// Transient unsaved-changes flag (not persisted).
    #[serde(skip)]
    dirty: bool,
}

impl Project {
    /// Create a project at the current format version with no markers, marked saved.
    pub fn new(name: impl Into<String>, timeline: Timeline) -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            name: name.into(),
            timeline,
            markers: MarkerSet::new(),
            dirty: false,
        }
    }

    /// Whether the project has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Flag the project as having unsaved changes (call after an edit).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clear the unsaved-changes flag (call after persisting).
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> Result<String, ProjectError> {
        serde_json::to_string_pretty(self).map_err(|error| ProjectError::Parse(error.to_string()))
    }

    /// Parse a project document, migrating older versions to the current format.
    ///
    /// Returns [`ProjectError::UnsupportedVersion`] for documents newer than this
    /// build, and [`ProjectError::Parse`] for malformed input — never a partial
    /// or silently-wrong project.
    pub fn from_json(json: &str) -> Result<Self, ProjectError> {
        let value: Value =
            serde_json::from_str(json).map_err(|error| ProjectError::Parse(error.to_string()))?;
        let version = match value.get("format_version") {
            None => 1,
            Some(value) => value.as_u64().ok_or_else(|| {
                ProjectError::Parse("format_version must be a positive integer".into())
            })?,
        };
        if version == 0 {
            return Err(ProjectError::Parse(
                "format_version must be at least 1".into(),
            ));
        }
        if version > u64::from(PROJECT_FORMAT_VERSION) {
            return Err(ProjectError::UnsupportedVersion(version));
        }

        let mut project: Project = serde_json::from_value(value)
            .map_err(|error| ProjectError::Parse(error.to_string()))?;
        if version < u64::from(PROJECT_FORMAT_VERSION) {
            project.migrate_from(version as u32);
        }
        project.validate_and_canonicalize()?;
        Ok(project)
    }

    fn migrate_from(&mut self, from: u32) {
        // v1 -> v2: `duration_frames` is authoritative; recompute it from clips so
        // stale or absent values from v1 documents are corrected.
        if from < 2 {
            self.timeline.recompute_duration();
        }
        self.format_version = PROJECT_FORMAT_VERSION;
    }

    fn validate_and_canonicalize(&mut self) -> Result<(), ProjectError> {
        if !self.timeline.frame_rate.is_finite() || self.timeline.frame_rate <= 0.0 {
            return Err(ProjectError::Parse(
                "timeline.frame_rate must be finite and positive".into(),
            ));
        }

        let mut track_ids = HashSet::new();
        for track in &self.timeline.tracks {
            if track.id.is_empty() || !track_ids.insert(track.id.as_str()) {
                return Err(ProjectError::Parse(
                    "track ids must be non-empty and unique".into(),
                ));
            }
            if !track.gain.is_finite() {
                return Err(ProjectError::Parse("track gain must be finite".into()));
            }

            let mut clip_ids = HashSet::new();
            for clip in &track.clips {
                if clip.id.is_empty() || !clip_ids.insert(clip.id.as_str()) {
                    return Err(ProjectError::Parse(format!(
                        "clip ids on track '{}' must be non-empty and unique",
                        track.id
                    )));
                }
                if clip.source.is_empty() {
                    return Err(ProjectError::Parse(format!(
                        "clip '{}' has an empty source",
                        clip.id
                    )));
                }
                let duration = clip
                    .end_frame
                    .checked_sub(clip.start_frame)
                    .ok_or_else(|| {
                        ProjectError::Parse(format!(
                            "clip '{}' has an invalid source range",
                            clip.id
                        ))
                    })?;
                if duration == 0 || clip.track_offset.checked_add(duration).is_none() {
                    return Err(ProjectError::Parse(format!(
                        "clip '{}' has an invalid or unrepresentable duration",
                        clip.id
                    )));
                }
                if !clip.opacity.is_finite() || !(0.0..=1.0).contains(&clip.opacity) {
                    return Err(ProjectError::Parse(format!(
                        "clip '{}' opacity must be in 0..=1",
                        clip.id
                    )));
                }
            }
        }

        // Persisted duration is derived data. Canonicalizing it on every load prevents a
        // stale current-version document from truncating playback or export.
        self.timeline.recompute_duration();
        Ok(())
    }

    /// Relink clips whose source equals `old_source` to `new_source`, returning
    /// the number of clips relinked.
    pub fn relink_source(&mut self, old_source: &str, new_source: &str) -> usize {
        let mut relinked = 0;
        for track in &mut self.timeline.tracks {
            for clip in &mut track.clips {
                if clip.source == old_source {
                    clip.source = new_source.to_string();
                    relinked += 1;
                }
            }
        }
        relinked
    }

    /// Every distinct media source the project references, sorted ascending — the set a
    /// media manager would check for offline media or relink in bulk.
    pub fn media_sources(&self) -> Vec<String> {
        let mut sources: Vec<String> = self
            .timeline
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter().map(|clip| clip.source.clone()))
            .collect();
        sources.sort();
        sources.dedup();
        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::{Automation, Interpolation, Keyframe};
    use crate::effects::{AnimatedEffect, AnimatedEffectStack};
    use crate::media::{ClipBlendMode, TimelineClip, TimelineTrack, TrackType};

    fn sample_timeline() -> Timeline {
        Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "Video".to_string(),
                track_type: TrackType::Video,
                enabled: true,
                gain: 1.0,
                clips: vec![TimelineClip {
                    id: "a".to_string(),
                    source: "a.mov".to_string(),
                    start_frame: 0,
                    end_frame: 50,
                    track_offset: 0,
                    opacity: 1.0,
                    opacity_curve: None,
                    blend_mode: Default::default(),
                    effects: Default::default(),
                    transform: Default::default(),
                    transform_curve: None,
                }],
            }],
            frame_rate: 30.0,
            duration_frames: 50,
        }
    }

    #[test]
    fn round_trips_through_json() {
        let project = Project::new("demo", sample_timeline());
        let json = project.to_json().unwrap();
        let restored = Project::from_json(&json).unwrap();
        assert_eq!(restored.format_version, PROJECT_FORMAT_VERSION);
        assert_eq!(restored.name, "demo");
        assert_eq!(project.to_json().unwrap(), restored.to_json().unwrap());
    }

    #[test]
    fn migrates_v1_document_and_recomputes_duration() {
        let v1 = r#"{
            "format_version": 1,
            "name": "old",
            "timeline": {
                "tracks": [{
                    "id": "v1", "name": "Video", "track_type": "Video",
                    "clips": [{ "id": "a", "source": "a.mov", "start_frame": 0, "end_frame": 50, "track_offset": 10 }]
                }],
                "frame_rate": 30.0,
                "duration_frames": 0
            }
        }"#;
        let project = Project::from_json(v1).unwrap();
        assert_eq!(project.format_version, PROJECT_FORMAT_VERSION);
        // duration recomputed from the clip: track_offset 10 + duration 50 = 60.
        assert_eq!(project.timeline.duration_frames, 60);
    }

    #[test]
    fn legacy_clip_without_layer_props_defaults_to_opaque_normal() {
        // A document predating per-clip opacity/blend_mode must still load, with the
        // new fields filled by their serde defaults.
        let json = r#"{
            "format_version": 2,
            "name": "legacy",
            "timeline": {
                "tracks": [{
                    "id": "v1", "name": "Video", "track_type": "Video",
                    "clips": [{ "id": "a", "source": "a.mov", "start_frame": 0, "end_frame": 50, "track_offset": 0 }]
                }],
                "frame_rate": 30.0,
                "duration_frames": 50
            }
        }"#;
        let project = Project::from_json(json).unwrap();
        let clip = &project.timeline.tracks[0].clips[0];
        assert_eq!(clip.opacity, 1.0);
        assert_eq!(clip.blend_mode, ClipBlendMode::Normal);
    }

    #[test]
    fn non_default_layer_props_survive_round_trip() {
        let mut timeline = sample_timeline();
        timeline.tracks[0].clips[0].opacity = 0.5;
        timeline.tracks[0].clips[0].blend_mode = ClipBlendMode::Multiply;
        let restored = Project::from_json(&Project::new("p", timeline).to_json().unwrap()).unwrap();
        let clip = &restored.timeline.tracks[0].clips[0];
        assert_eq!(clip.opacity, 0.5);
        assert_eq!(clip.blend_mode, ClipBlendMode::Multiply);
    }

    #[test]
    fn markers_persist_and_default_empty() {
        use crate::markers::Marker;
        // A document without markers loads with an empty set.
        let legacy = r#"{
            "format_version": 2, "name": "p",
            "timeline": { "tracks": [], "frame_rate": 30.0, "duration_frames": 0 }
        }"#;
        assert!(Project::from_json(legacy).unwrap().markers.is_empty());

        // Markers survive save/load.
        let mut project = Project::new("p", sample_timeline());
        project.markers.add(Marker::new(120, "chapter 2"));
        let restored = Project::from_json(&project.to_json().unwrap()).unwrap();
        assert_eq!(restored.markers.len(), 1);
        assert_eq!(restored.markers.nearest(120).map(|m| m.frame), Some(120));
    }

    #[test]
    fn tracks_unsaved_changes_transiently() {
        let mut project = Project::new("p", sample_timeline());
        assert!(!project.is_dirty());
        project.mark_dirty();
        assert!(project.is_dirty());
        project.mark_saved();
        assert!(!project.is_dirty());

        // The dirty flag is not serialized; a loaded project starts clean.
        let json = project.to_json().unwrap();
        assert!(!json.contains("dirty"));
        let loaded = Project::from_json(&json).unwrap();
        assert!(!loaded.is_dirty());
    }

    #[test]
    fn missing_version_is_treated_as_v1() {
        let json = r#"{
            "name": "no version",
            "timeline": { "tracks": [], "frame_rate": 24.0, "duration_frames": 0 }
        }"#;
        let project = Project::from_json(json).unwrap();
        assert_eq!(project.format_version, PROJECT_FORMAT_VERSION);
    }

    #[test]
    fn rejects_future_version() {
        let json = r#"{ "format_version": 999, "name": "future", "timeline": { "tracks": [], "frame_rate": 30.0, "duration_frames": 0 } }"#;
        assert_eq!(
            Project::from_json(json).unwrap_err(),
            ProjectError::UnsupportedVersion(999)
        );
        let wrapped = r#"{ "format_version": 4294967298, "name": "future", "timeline": { "tracks": [], "frame_rate": 30.0, "duration_frames": 0 } }"#;
        assert_eq!(
            Project::from_json(wrapped).unwrap_err(),
            ProjectError::UnsupportedVersion(4_294_967_298)
        );
    }

    #[test]
    fn rejects_malformed_or_zero_version() {
        for version in [r#""2""#, "0", "-1", "2.5"] {
            let json = format!(
                r#"{{ "format_version": {version}, "timeline": {{ "tracks": [], "frame_rate": 30.0, "duration_frames": 0 }} }}"#
            );
            assert!(matches!(
                Project::from_json(&json),
                Err(ProjectError::Parse(_))
            ));
        }
    }

    #[test]
    fn rejects_corrupt_json() {
        assert!(matches!(
            Project::from_json("{ not valid json"),
            Err(ProjectError::Parse(_))
        ));
    }

    #[test]
    fn rejects_invalid_timeline_invariants() {
        let cases = [
            r#"{"format_version":2,"timeline":{"tracks":[],"frame_rate":0,"duration_frames":0}}"#,
            r#"{"format_version":2,"timeline":{"tracks":[{"id":"v","name":"v","track_type":"Video","clips":[{"id":"a","source":"a.mov","start_frame":10,"end_frame":5,"track_offset":0}]}],"frame_rate":30,"duration_frames":0}}"#,
            r#"{"format_version":2,"timeline":{"tracks":[{"id":"v","name":"v","track_type":"Video","clips":[{"id":"a","source":"a.mov","start_frame":0,"end_frame":5,"track_offset":0},{"id":"a","source":"b.mov","start_frame":0,"end_frame":5,"track_offset":5}]}],"frame_rate":30,"duration_frames":10}}"#,
            r#"{"format_version":2,"timeline":{"tracks":[{"id":"v","name":"v","track_type":"Video","clips":[]},{"id":"v","name":"duplicate","track_type":"Video","clips":[]}],"frame_rate":30,"duration_frames":0}}"#,
        ];
        for json in cases {
            assert!(matches!(
                Project::from_json(json),
                Err(ProjectError::Parse(_))
            ));
        }
    }

    #[test]
    fn current_documents_recompute_stale_duration() {
        let json = r#"{
            "format_version":2,
            "timeline":{"tracks":[{"id":"v","name":"v","track_type":"Video","clips":[
                {"id":"a","source":"a.mov","start_frame":0,"end_frame":10,"track_offset":20}
            ]}],"frame_rate":30,"duration_frames":1}
        }"#;
        assert_eq!(
            Project::from_json(json).unwrap().timeline.duration_frames,
            30
        );
    }

    #[test]
    fn relinks_matching_sources() {
        let mut project = Project::new("demo", sample_timeline());
        assert_eq!(project.relink_source("a.mov", "b.mov"), 1);
        assert_eq!(project.timeline.tracks[0].clips[0].source, "b.mov");
        assert_eq!(project.relink_source("missing.mov", "x.mov"), 0);
    }

    fn clip_from(id: &str, source: &str) -> TimelineClip {
        TimelineClip {
            id: id.to_string(),
            source: source.to_string(),
            start_frame: 0,
            end_frame: 50,
            track_offset: 0,
            opacity: 1.0,
            opacity_curve: None,
            blend_mode: Default::default(),
            effects: Default::default(),
            transform: Default::default(),
            transform_curve: None,
        }
    }

    fn video_track(id: &str, clips: Vec<TimelineClip>) -> TimelineTrack {
        TimelineTrack {
            id: id.to_string(),
            name: id.to_string(),
            track_type: TrackType::Video,
            clips,
            enabled: true,
            gain: 1.0,
        }
    }

    #[test]
    fn lists_distinct_media_sources_sorted() {
        let timeline = Timeline {
            tracks: vec![
                video_track("v1", vec![clip_from("a", "b.mov"), clip_from("b", "a.mov")]),
                // A duplicate source on another track collapses to one entry.
                video_track("v2", vec![clip_from("c", "b.mov")]),
            ],
            frame_rate: 30.0,
            duration_frames: 50,
        };
        let project = Project::new("p", timeline);
        assert_eq!(
            project.media_sources(),
            vec!["a.mov".to_string(), "b.mov".to_string()]
        );
    }

    #[test]
    fn keyframed_effects_survive_a_round_trip() {
        let mut stops = Automation::constant(0.0);
        stops.add(Keyframe {
            time_ms: 0,
            value: 0.0,
            interpolation: Interpolation::Linear,
        });
        stops.add(Keyframe {
            time_ms: 1000,
            value: 1.5,
            interpolation: Interpolation::Smooth,
        });
        let mut timeline = sample_timeline();
        timeline.tracks[0].clips[0].effects = AnimatedEffectStack {
            effects: vec![AnimatedEffect::Exposure { stops }],
        };

        let json = Project::new("p", timeline.clone()).to_json().unwrap();
        let restored = Project::from_json(&json).unwrap();
        assert_eq!(
            restored.timeline.tracks[0].clips[0].effects,
            timeline.tracks[0].clips[0].effects
        );
    }

    #[test]
    fn legacy_clip_without_effects_loads_with_empty_stack() {
        // A document predating the effects field must still load, with an empty stack.
        let json = r#"{
            "format_version": 2, "name": "legacy",
            "timeline": {
                "tracks": [{
                    "id": "v1", "name": "Video", "track_type": "Video",
                    "clips": [{ "id": "a", "source": "a.mov", "start_frame": 0, "end_frame": 50, "track_offset": 0 }]
                }],
                "frame_rate": 30.0, "duration_frames": 50
            }
        }"#;
        let project = Project::from_json(json).unwrap();
        assert!(project.timeline.tracks[0].clips[0].effects.is_empty());
    }
}
