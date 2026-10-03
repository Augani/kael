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

    /// The first contiguous logical fragment and its actual native rectangle.
    /// Preserve source order instead of choosing the leftmost bidi rectangle.
    /// Whole native clusters are retained, and missing requested source spans
    /// or disjoint physical geometry end the fragment. No new shaping occurs.
    pub fn first_fragment_for_bytes(
        &self,
        range: Range<usize>,
    ) -> Option<(Range<usize>, Range<Pixels>)> {
        if range.start >= range.end {
            return None;
        }
        let index = self
            .clusters
            .partition_point(|cluster| cluster.bytes.end <= range.start);
        let first = self.clusters.get(index)?;
        if !first.bytes.contains(&range.start) {
            return None;
        }
        let mut actual = first.bytes.clone();
        let mut rectangle = first.leading.min(first.trailing)..first.leading.max(first.trailing);
        for next in &self.clusters[index + 1..] {
            if next.bytes.start >= range.end || next.bytes.start != actual.end {
                break;
            }
            let left = next.leading.min(next.trailing);
            let right = next.leading.max(next.trailing);
            if left > rectangle.end + crate::px(0.01) || right + crate::px(0.01) < rectangle.start {
                break;
            }
            actual.end = next.bytes.end;
            rectangle.start = rectangle.start.min(left);
            rectangle.end = rectangle.end.max(right);
        }
        Some((actual, rectangle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::px;

    #[test]
    fn first_fragment_follows_source_order_and_stops_at_physical_gaps() {
        let geometry = LineTextGeometry::new(
            "abcd",
            vec![
                ShapedTextCluster {
                    bytes: 0..1,
                    leading: px(90.0),
                    trailing: px(80.0),
                    right_to_left: true,
                },
                ShapedTextCluster {
                    bytes: 1..2,
                    leading: px(80.0),
                    trailing: px(70.0),
                    right_to_left: true,
                },
                ShapedTextCluster {
                    bytes: 2..3,
                    leading: px(10.0),
                    trailing: px(20.0),
                    right_to_left: false,
                },
                ShapedTextCluster {
                    bytes: 3..4,
                    leading: px(20.0),
                    trailing: px(30.0),
                    right_to_left: false,
                },
            ],
            px(30.0),
        )
        .unwrap();
        assert_eq!(
            geometry.first_fragment_for_bytes(0..4),
            Some((0..2, px(70.0)..px(90.0)))
        );
        assert_eq!(
            geometry.first_fragment_for_bytes(1..4),
            Some((1..2, px(70.0)..px(80.0)))
        );
        assert_eq!(
            geometry.first_fragment_for_bytes(2..4),
            Some((2..4, px(10.0)..px(30.0)))
        );
    }

    #[test]
    fn first_fragment_preserves_clusters_and_stops_at_missing_source_coverage() {
        let geometry = LineTextGeometry::new(
            "e\u{301}ab",
            vec![
                ShapedTextCluster {
                    bytes: 0..3,
                    leading: px(0.0),
                    trailing: px(10.0),
                    right_to_left: false,
                },
                ShapedTextCluster {
                    bytes: 4..5,
                    leading: px(10.0),
                    trailing: px(20.0),
                    right_to_left: false,
                },
            ],
            px(20.0),
        )
        .unwrap();
        assert_eq!(
            geometry.first_fragment_for_bytes(1..5),
            Some((0..3, px(0.0)..px(10.0)))
        );
        assert_eq!(geometry.first_fragment_for_bytes(3..5), None);
        assert_eq!(geometry.first_fragment_for_bytes(0..0), None);
    }
}
