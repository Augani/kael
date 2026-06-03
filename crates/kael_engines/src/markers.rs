//! Timeline markers — named cue / chapter / comment points used for navigation and
//! snapping.
//!
//! [`MarkerSet`] keeps a collection of [`Marker`]s. Its queries (next/previous/nearest,
//! in-range) do not assume any storage order, so the set survives serialization and
//! manual edits without an explicit re-sort.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// A named marker pinned to a timeline frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Timeline frame the marker sits on.
    pub frame: u64,
    /// Human-readable label (chapter title, comment).
    pub label: String,
    /// Optional display color as linear `[r, g, b, a]`.
    #[serde(default)]
    pub color: Option<[f32; 4]>,
}

impl Marker {
    /// Create a marker at `frame` with `label` and no color.
    pub fn new(frame: u64, label: impl Into<String>) -> Self {
        Self {
            frame,
            label: label.into(),
            color: None,
        }
    }

    /// Set the marker's display color.
    pub fn with_color(mut self, color: [f32; 4]) -> Self {
        self.color = Some(color);
        self
    }
}

/// An ordered collection of timeline markers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarkerSet {
    markers: Vec<Marker>,
}

impl MarkerSet {
    /// An empty marker set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from existing markers, kept sorted by frame for tidy iteration.
    pub fn from_markers(mut markers: Vec<Marker>) -> Self {
        markers.sort_by_key(|marker| marker.frame);
        Self { markers }
    }

    /// Add a marker, keeping the set ordered by frame.
    pub fn add(&mut self, marker: Marker) {
        let index = self
            .markers
            .partition_point(|existing| existing.frame <= marker.frame);
        self.markers.insert(index, marker);
    }

    /// Remove every marker sitting exactly on `frame`. Returns how many were removed.
    pub fn remove_at(&mut self, frame: u64) -> usize {
        let before = self.markers.len();
        self.markers.retain(|marker| marker.frame != frame);
        before - self.markers.len()
    }

    /// Number of markers.
    pub fn len(&self) -> usize {
        self.markers.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.markers.is_empty()
    }

    /// Iterate markers in frame order.
    pub fn iter(&self) -> impl Iterator<Item = &Marker> {
        self.markers.iter()
    }

    /// The sorted, de-duplicated marker frames — usable as extra snap points.
    pub fn frames(&self) -> Vec<u64> {
        let mut frames: Vec<u64> = self.markers.iter().map(|marker| marker.frame).collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// The first marker strictly after `frame` (jump-to-next).
    pub fn next_after(&self, frame: u64) -> Option<&Marker> {
        self.markers
            .iter()
            .filter(|marker| marker.frame > frame)
            .min_by_key(|marker| marker.frame)
    }

    /// The last marker strictly before `frame` (jump-to-previous).
    pub fn prev_before(&self, frame: u64) -> Option<&Marker> {
        self.markers
            .iter()
            .filter(|marker| marker.frame < frame)
            .max_by_key(|marker| marker.frame)
    }

    /// The marker closest to `frame` (ties favor the earlier frame).
    pub fn nearest(&self, frame: u64) -> Option<&Marker> {
        self.markers
            .iter()
            .min_by_key(|marker| (frame.abs_diff(marker.frame), marker.frame))
    }

    /// Markers whose frame falls within `range` (half-open), in frame order.
    pub fn in_range(&self, range: Range<u64>) -> impl Iterator<Item = &Marker> {
        self.markers
            .iter()
            .filter(move |marker| range.contains(&marker.frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> MarkerSet {
        let mut set = MarkerSet::new();
        set.add(Marker::new(100, "c"));
        set.add(Marker::new(20, "a"));
        set.add(Marker::new(60, "b"));
        set
    }

    #[test]
    fn add_keeps_frame_order() {
        let set = populated();
        let frames: Vec<u64> = set.iter().map(|marker| marker.frame).collect();
        assert_eq!(frames, vec![20, 60, 100]);
    }

    #[test]
    fn navigation_finds_next_and_previous() {
        let set = populated();
        assert_eq!(set.next_after(20).map(|m| m.frame), Some(60));
        assert_eq!(set.next_after(60).map(|m| m.frame), Some(100));
        assert_eq!(set.next_after(100), None);
        assert_eq!(set.prev_before(100).map(|m| m.frame), Some(60));
        assert_eq!(set.prev_before(20), None);
    }

    #[test]
    fn nearest_picks_closest_marker() {
        let set = populated();
        assert_eq!(set.nearest(65).map(|m| m.frame), Some(60));
        assert_eq!(set.nearest(85).map(|m| m.frame), Some(100));
        // Equidistant between 60 and 100 -> the earlier one.
        assert_eq!(set.nearest(80).map(|m| m.frame), Some(60));
        assert_eq!(MarkerSet::new().nearest(0), None);
    }

    #[test]
    fn in_range_filters_half_open() {
        let set = populated();
        let frames: Vec<u64> = set.in_range(20..100).map(|m| m.frame).collect();
        assert_eq!(frames, vec![20, 60]);
    }

    #[test]
    fn remove_at_drops_matching_frames() {
        let mut set = populated();
        set.add(Marker::new(60, "dup"));
        assert_eq!(set.remove_at(60), 2);
        assert_eq!(set.frames(), vec![20, 100]);
        assert_eq!(set.remove_at(999), 0);
    }

    #[test]
    fn queries_are_order_independent_after_round_trip() {
        let set = populated();
        let json = serde_json::to_string(&set).unwrap();
        let restored: MarkerSet = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.nearest(65).map(|m| m.frame), Some(60));
        assert_eq!(restored.next_after(20).map(|m| m.frame), Some(60));
        assert_eq!(restored, set);
    }
}
