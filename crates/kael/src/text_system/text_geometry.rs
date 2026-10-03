//! Optional native cluster metrics for controls that expose editable text.

use crate::Pixels;
use std::ops::Range;

/// Actual native caret edges of one shaped cluster in a single hard line.
/// Multiple scalars may occupy one ligature or grapheme cluster. Backends must
/// preserve that cluster rectangle rather than inventing scalar subdivisions.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedTextCluster {
    /// UTF-8 source bytes belonging to the cluster.
    pub bytes: Range<usize>,
    /// Physical x coordinate at the logical leading edge.
    pub leading: Pixels,
    /// Physical x coordinate at the logical trailing edge.
    pub trailing: Pixels,
    /// Whether logical order runs from right to left.
    pub right_to_left: bool,
}

/// Bounded native geometry requested separately from the shared glyph cache.
/// Clusters are sorted in source order and only cover requested byte spans.
#[derive(Clone, Debug)]
pub struct LineTextGeometry {
    /// Native clusters intersecting the caller's requested spans.
    pub clusters: Vec<ShapedTextCluster>,
    /// Native physical caret coordinate at the end of the complete hard line.
    pub end_caret: Pixels,
}

impl LineTextGeometry {
    /// Validate requested UTF-8 spans before entering a native text API.
    pub fn requested_ranges_valid(text: &str, byte_ranges: &[Range<usize>]) -> bool {
        byte_ranges.iter().all(|range| {
            range.start <= range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
    }
    /// Validate UTF-8 boundaries and finite native coordinates. Equal source
    /// clusters emitted by multiple fallback glyphs are combined into their
    /// actual union. Other overlapping source spans are rejected.
    pub fn new(
        text: &str,
        mut clusters: Vec<ShapedTextCluster>,
        end_caret: Pixels,
    ) -> Option<Self> {
        if !f32::from(end_caret).is_finite() {
            return None;
        }
        clusters.sort_unstable_by_key(|cluster| (cluster.bytes.start, cluster.bytes.end));
        let mut unique: Vec<ShapedTextCluster> = Vec::with_capacity(clusters.len());
        for cluster in clusters {
            if cluster.bytes.start >= cluster.bytes.end
                || cluster.bytes.end > text.len()
                || !text.is_char_boundary(cluster.bytes.start)
                || !text.is_char_boundary(cluster.bytes.end)
                || !f32::from(cluster.leading).is_finite()
                || !f32::from(cluster.trailing).is_finite()
                || (cluster.right_to_left && cluster.leading < cluster.trailing)
                || (!cluster.right_to_left && cluster.leading > cluster.trailing)
            {
                return None;
            }
            if let Some(previous) = unique.last_mut() {
                if previous.bytes == cluster.bytes
                    && previous.right_to_left == cluster.right_to_left
                {
                    if cluster.right_to_left {
                        previous.leading = previous.leading.max(cluster.leading);
                        previous.trailing = previous.trailing.min(cluster.trailing);
                    } else {
                        previous.leading = previous.leading.min(cluster.leading);
                        previous.trailing = previous.trailing.max(cluster.trailing);
                    }
                    continue;
                }
                if previous.bytes.end > cluster.bytes.start {
                    return None;
                }
            }
            unique.push(cluster);
        }
        Some(Self {
            clusters: unique,
            end_caret,
        })
    }

    /// Resolve a source scalar to its actual native cluster.
    pub fn cluster_for_byte(&self, byte: usize) -> Option<&ShapedTextCluster> {
        let index = self
            .clusters
            .partition_point(|cluster| cluster.bytes.end <= byte);
        self.clusters
            .get(index)
            .filter(|cluster| cluster.bytes.contains(&byte))
    }

    /// Native caret with forward affinity; only requested bytes and EOF are
    /// available. Interior cluster scalars share its native leading caret.
    pub fn caret_for_byte(&self, byte: usize, line_length: usize) -> Option<Pixels> {
        if byte == line_length {
            return Some(self.end_caret);
        }
        self.cluster_for_byte(byte).map(|cluster| cluster.leading)
    }

    /// Closest native cluster caret to a physical point. A ligature cluster
    /// keeps its real endpoints; no invented interior caret is returned.
    pub fn closest_byte_for_x(&self, x: Pixels, line_length: usize) -> usize {
        self.clusters
            .iter()
            .flat_map(|cluster| {
                [
                    (cluster.bytes.start, cluster.leading),
                    (cluster.bytes.end, cluster.trailing),
                ]
            })
            .chain(std::iter::once((line_length, self.end_caret)))
            .min_by(|a, b| {
                (a.1 - x)
                    .abs()
                    .partial_cmp(&(b.1 - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or(0, |(byte, _)| byte)
    }

    /// Physical fragments intersecting a logical selection, merged only when
    /// their actual cluster rectangles touch. Bidi selections can be disjoint.
    pub fn rectangles_for_bytes(&self, range: Range<usize>) -> Vec<Range<Pixels>> {
        let mut rectangles = self
            .clusters
            .iter()
            .filter(|cluster| range.start < cluster.bytes.end && cluster.bytes.start < range.end)
            .map(|cluster| {
                cluster.leading.min(cluster.trailing)..cluster.leading.max(cluster.trailing)
            })
            .collect::<Vec<_>>();
        rectangles.sort_unstable_by(|a, b| {
            a.start
                .partial_cmp(&b.start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut merged: Vec<Range<Pixels>> = Vec::new();
        for rectangle in rectangles {
            if let Some(previous) = merged.last_mut()
                && rectangle.start <= previous.end + crate::px(0.01)
            {
                previous.end = previous.end.max(rectangle.end);
                continue;
            }
            merged.push(rectangle);
        }
        merged
    }
}
