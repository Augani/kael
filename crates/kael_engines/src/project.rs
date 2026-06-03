//! Versioned, relinkable project document model.
//!
//! Wraps a [`Timeline`](crate::media::Timeline) in a format-versioned envelope so
//! projects can evolve safely across releases: older documents are migrated to
//! the current format on load, newer-than-supported documents are rejected
//! rather than misread, and media can be relinked when sources move.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::media::Timeline;

/// The current on-disk project format version.
pub const PROJECT_FORMAT_VERSION: u32 = 2;

/// An error reading or migrating a project document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectError {
    /// The JSON could not be parsed or serialized.
    Parse(String),
    /// The document's format version is newer than this build supports.
    UnsupportedVersion(u32),
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
}

impl Project {
    /// Create a project at the current format version.
    pub fn new(name: impl Into<String>, timeline: Timeline) -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            name: name.into(),
            timeline,
        }
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
        let version = value
            .get("format_version")
            .and_then(Value::as_u64)
            .unwrap_or(1) as u32;
        if version > PROJECT_FORMAT_VERSION {
            return Err(ProjectError::UnsupportedVersion(version));
        }

        let mut project: Project = serde_json::from_value(value)
            .map_err(|error| ProjectError::Parse(error.to_string()))?;
        if version < PROJECT_FORMAT_VERSION {
            project.migrate_from(version);
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{TimelineClip, TimelineTrack, TrackType};

    fn sample_timeline() -> Timeline {
        Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "Video".to_string(),
                track_type: TrackType::Video,
                clips: vec![TimelineClip {
                    id: "a".to_string(),
                    source: "a.mov".to_string(),
                    start_frame: 0,
                    end_frame: 50,
                    track_offset: 0,
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
    }

    #[test]
    fn rejects_corrupt_json() {
        assert!(matches!(
            Project::from_json("{ not valid json"),
            Err(ProjectError::Parse(_))
        ));
    }

    #[test]
    fn relinks_matching_sources() {
        let mut project = Project::new("demo", sample_timeline());
        assert_eq!(project.relink_source("a.mov", "b.mov"), 1);
        assert_eq!(project.timeline.tracks[0].clips[0].source, "b.mov");
        assert_eq!(project.relink_source("missing.mov", "x.mov"), 0);
    }
}
