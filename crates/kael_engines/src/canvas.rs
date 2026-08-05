//! Canvas and design workload engine.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A 2D point on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanvasPoint {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// An axis-aligned rectangle on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanvasRect {
    /// Left edge x coordinate.
    pub x: f64,
    /// Top edge y coordinate.
    pub y: f64,
    /// Width of the rectangle.
    pub width: f64,
    /// Height of the rectangle.
    pub height: f64,
}

/// The type of a path segment command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathSegmentType {
    /// Move the pen to a new position.
    MoveTo,
    /// Draw a straight line.
    LineTo,
    /// Draw a quadratic Bezier curve.
    QuadTo,
    /// Draw a cubic Bezier curve.
    CubicTo,
    /// Close the current sub-path.
    Close,
}

/// A single segment of a vector path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathSegment {
    /// Segment command type.
    pub segment_type: PathSegmentType,
    /// Control and endpoint coordinates for this segment.
    pub points: Vec<CanvasPoint>,
}

/// A vector path composed of segments.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VectorPath {
    /// Ordered segments making up the path.
    pub segments: Vec<PathSegment>,
    /// Whether the path forms a closed shape.
    pub closed: bool,
}

impl VectorPath {
    /// Create a new empty vector path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a segment to the path.
    pub fn add_segment(&mut self, segment: PathSegment) {
        self.segments.push(segment);
    }

    /// Compute the axis-aligned bounding box of all points in the path.
    /// Returns `None` if the path contains no points.
    pub fn bounds(&self) -> Option<CanvasRect> {
        let mut points = self
            .segments
            .iter()
            .flat_map(|s| s.points.iter())
            .filter(|point| point.x.is_finite() && point.y.is_finite());
        let first = points.next()?;
        let (mut min_x, mut min_y) = (first.x, first.y);
        let (mut max_x, mut max_y) = (first.x, first.y);
        for p in points {
            if p.x < min_x {
                min_x = p.x;
            }
            if p.y < min_y {
                min_y = p.y;
            }
            if p.x > max_x {
                max_x = p.x;
            }
            if p.y > max_y {
                max_y = p.y;
            }
        }
        Some(CanvasRect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        })
    }

    /// Total number of points across all segments.
    pub fn point_count(&self) -> usize {
        self.segments.iter().fold(0usize, |total, segment| {
            total.saturating_add(segment.points.len())
        })
    }

    /// Translate all points by the given offset.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        let dx = if dx.is_finite() { dx } else { 0.0 };
        let dy = if dy.is_finite() { dy } else { 0.0 };
        for seg in &mut self.segments {
            for p in &mut seg.points {
                let x = p.x + dx;
                let y = p.y + dy;
                if x.is_finite() {
                    p.x = x;
                }
                if y.is_finite() {
                    p.y = y;
                }
            }
        }
    }
}

/// Supported image export formats for canvas content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportImageFormat {
    /// Portable Network Graphics.
    Png,
    /// Scalable Vector Graphics.
    Svg,
    /// Portable Document Format.
    Pdf,
    /// JPEG image.
    Jpeg,
}

/// Configuration for exporting canvas content to an image file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasExport {
    /// Target image format.
    pub format: ExportImageFormat,
    /// Export width in pixels.
    pub width: u32,
    /// Export height in pixels.
    pub height: u32,
    /// Dots per inch for the export.
    pub dpi: f64,
    /// Optional background color (e.g. "#ffffff").
    pub background: Option<String>,
}

impl CanvasExport {
    /// Validate the export configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.width == 0 {
            anyhow::bail!("width must be greater than zero");
        }
        if self.height == 0 {
            anyhow::bail!("height must be greater than zero");
        }
        if !self.dpi.is_finite() || self.dpi <= 0.0 {
            anyhow::bail!("dpi must be positive");
        }
        Ok(())
    }
}

/// A coordinate identifying a tile in a tiled canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    /// Horizontal tile index.
    pub x: i32,
    /// Vertical tile index.
    pub y: i32,
    /// Zoom level.
    pub zoom: u8,
}

#[derive(Debug)]
struct CachedTile {
    data: Vec<u8>,
    order: u64,
}

/// Cache for rendered tile data, bounded by a maximum byte budget with
/// least-recently-used eviction (driven by [`TileCache::insert`] and
/// [`TileCache::touch`]).
#[derive(Debug)]
pub struct TileCache {
    tiles: HashMap<TileCoord, CachedTile>,
    max_bytes: usize,
    current_bytes: usize,
    clock: u64,
}

impl Default for TileCache {
    fn default() -> Self {
        Self::with_max_bytes(Self::DEFAULT_MAX_BYTES)
    }
}

impl TileCache {
    /// Default resident tile-data budget: 256 MiB.
    pub const DEFAULT_MAX_BYTES: usize = 256 * 1024 * 1024;

    /// Create a cache with the default 256 MiB resident-data budget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a tile cache bounded to at most `max_bytes` of tile data. Inserting beyond
    /// the budget evicts least-recently-used tiles until the data fits.
    pub fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            tiles: HashMap::new(),
            max_bytes,
            current_bytes: 0,
            clock: 0,
        }
    }

    fn next_order(&mut self) -> u64 {
        if self.clock == u64::MAX {
            let mut by_age: Vec<_> = self
                .tiles
                .iter()
                .map(|(coord, tile)| (*coord, tile.order))
                .collect();
            by_age.sort_unstable_by_key(|(_, order)| *order);
            for (index, (coord, _)) in by_age.into_iter().enumerate() {
                if let Some(tile) = self.tiles.get_mut(&coord) {
                    tile.order = u64::try_from(index).unwrap_or(u64::MAX - 1) + 1;
                }
            }
            self.clock = u64::try_from(self.tiles.len()).unwrap_or(u64::MAX - 1);
        }
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    /// Insert tile data at the given coordinate, evicting LRU tiles if over budget.
    pub fn insert(&mut self, coord: TileCoord, data: Vec<u8>) {
        let bytes = data.len();
        if bytes > self.max_bytes {
            return;
        }
        let order = self.next_order();
        if let Some(previous) = self.tiles.insert(coord, CachedTile { data, order }) {
            self.current_bytes = self.current_bytes.saturating_sub(previous.data.len());
        }
        self.current_bytes = self.current_bytes.saturating_add(bytes);
        self.evict_to_budget();
    }

    /// Get cached tile data.
    pub fn get(&self, coord: &TileCoord) -> Option<&Vec<u8>> {
        self.tiles.get(coord).map(|tile| &tile.data)
    }

    /// Mark a tile as most-recently-used so it is evicted last. Returns true if present.
    pub fn touch(&mut self, coord: &TileCoord) -> bool {
        let order = self.next_order();
        match self.tiles.get_mut(coord) {
            Some(tile) => {
                tile.order = order;
                true
            }
            None => false,
        }
    }

    /// Remove a single tile from the cache. Returns true if the tile existed.
    pub fn invalidate(&mut self, coord: &TileCoord) -> bool {
        match self.tiles.remove(coord) {
            Some(tile) => {
                self.current_bytes = self.current_bytes.saturating_sub(tile.data.len());
                true
            }
            None => false,
        }
    }

    /// Remove all cached tiles.
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.current_bytes = 0;
    }

    /// Total bytes of tile data currently cached.
    pub fn byte_len(&self) -> usize {
        self.current_bytes
    }

    /// Maximum resident tile-data budget in bytes.
    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Number of tiles currently cached.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Returns true if no tiles are cached.
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    fn evict_to_budget(&mut self) {
        while self.current_bytes > self.max_bytes && !self.tiles.is_empty() {
            let Some(coord) = self
                .tiles
                .iter()
                .min_by_key(|(_, tile)| tile.order)
                .map(|(coord, _)| *coord)
            else {
                break;
            };
            if let Some(tile) = self.tiles.remove(&coord) {
                self.current_bytes = self.current_bytes.saturating_sub(tile.data.len());
            }
        }
    }

    /// Return the coordinates of tiles visible within a viewport at the given zoom level.
    pub fn visible_tiles(&self, viewport: &CanvasRect, zoom: u8) -> Vec<TileCoord> {
        const MAX_VISIBLE_TILES: u64 = 1_048_576;
        let tile_size = 256.0;
        if !viewport.x.is_finite()
            || !viewport.y.is_finite()
            || !viewport.width.is_finite()
            || !viewport.height.is_finite()
            || viewport.width <= 0.0
            || viewport.height <= 0.0
        {
            return Vec::new();
        }
        let right = viewport.x + viewport.width;
        let bottom = viewport.y + viewport.height;
        if !right.is_finite() || !bottom.is_finite() {
            return Vec::new();
        }
        let start_x = (viewport.x / tile_size).floor() as i32;
        let start_y = (viewport.y / tile_size).floor() as i32;
        let end_x = (right / tile_size).ceil() as i32;
        let end_y = (bottom / tile_size).ceil() as i32;
        let width = i64::from(end_x).saturating_sub(i64::from(start_x));
        let height = i64::from(end_y).saturating_sub(i64::from(start_y));
        let Ok(width) = u64::try_from(width) else {
            return Vec::new();
        };
        let Ok(height) = u64::try_from(height) else {
            return Vec::new();
        };
        let Some(tile_count) = width.checked_mul(height) else {
            return Vec::new();
        };
        if tile_count > MAX_VISIBLE_TILES {
            return Vec::new();
        }

        let mut coords = Vec::new();
        if coords
            .try_reserve_exact(usize::try_from(tile_count).unwrap_or(usize::MAX))
            .is_err()
        {
            return Vec::new();
        }
        for tx in start_x..end_x {
            for ty in start_y..end_y {
                coords.push(TileCoord { x: tx, y: ty, zoom });
            }
        }
        coords
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> CanvasPoint {
        CanvasPoint { x, y }
    }

    #[test]
    fn vector_path_add_segment_and_count() {
        let mut path = VectorPath::new();
        path.add_segment(PathSegment {
            segment_type: PathSegmentType::MoveTo,
            points: vec![pt(0.0, 0.0)],
        });
        path.add_segment(PathSegment {
            segment_type: PathSegmentType::LineTo,
            points: vec![pt(10.0, 20.0)],
        });
        assert_eq!(path.segments.len(), 2);
        assert_eq!(path.point_count(), 2);
    }

    #[test]
    fn vector_path_bounds() {
        let mut path = VectorPath::new();
        path.add_segment(PathSegment {
            segment_type: PathSegmentType::MoveTo,
            points: vec![pt(5.0, 10.0)],
        });
        path.add_segment(PathSegment {
            segment_type: PathSegmentType::LineTo,
            points: vec![pt(15.0, 30.0)],
        });
        let b = path.bounds().unwrap();
        assert!((b.x - 5.0).abs() < f64::EPSILON);
        assert!((b.y - 10.0).abs() < f64::EPSILON);
        assert!((b.width - 10.0).abs() < f64::EPSILON);
        assert!((b.height - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn vector_path_bounds_empty() {
        let path = VectorPath::new();
        assert!(path.bounds().is_none());
    }

    #[test]
    fn vector_path_translate() {
        let mut path = VectorPath::new();
        path.add_segment(PathSegment {
            segment_type: PathSegmentType::MoveTo,
            points: vec![pt(1.0, 2.0)],
        });
        path.translate(10.0, -5.0);
        let p = &path.segments[0].points[0];
        assert!((p.x - 11.0).abs() < f64::EPSILON);
        assert!((p.y - (-3.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn canvas_export_validate_ok() {
        let exp = CanvasExport {
            format: ExportImageFormat::Png,
            width: 1024,
            height: 768,
            dpi: 72.0,
            background: Some("#ffffff".into()),
        };
        assert!(exp.validate().is_ok());
    }

    #[test]
    fn canvas_export_validate_zero_width() {
        let exp = CanvasExport {
            format: ExportImageFormat::Svg,
            width: 0,
            height: 100,
            dpi: 72.0,
            background: None,
        };
        assert!(exp.validate().is_err());
    }

    #[test]
    fn canvas_export_validate_bad_dpi() {
        let exp = CanvasExport {
            format: ExportImageFormat::Pdf,
            width: 100,
            height: 100,
            dpi: 0.0,
            background: None,
        };
        assert!(exp.validate().is_err());
        let mut non_finite = exp;
        non_finite.dpi = f64::NAN;
        assert!(non_finite.validate().is_err());
    }

    #[test]
    fn tile_cache_insert_and_get() {
        let mut cache = TileCache::new();
        let coord = TileCoord {
            x: 1,
            y: 2,
            zoom: 3,
        };
        cache.insert(coord, vec![42]);
        assert_eq!(cache.get(&coord).unwrap(), &vec![42]);
        assert!(
            cache
                .get(&TileCoord {
                    x: 0,
                    y: 0,
                    zoom: 0
                })
                .is_none()
        );
    }

    #[test]
    fn tile_cache_invalidate() {
        let mut cache = TileCache::new();
        let coord = TileCoord {
            x: 0,
            y: 0,
            zoom: 1,
        };
        cache.insert(coord, vec![1]);
        assert!(cache.invalidate(&coord));
        assert!(!cache.invalidate(&coord));
    }

    #[test]
    fn tile_cache_clear() {
        let mut cache = TileCache::new();
        cache.insert(
            TileCoord {
                x: 0,
                y: 0,
                zoom: 0,
            },
            vec![],
        );
        cache.insert(
            TileCoord {
                x: 1,
                y: 0,
                zoom: 0,
            },
            vec![],
        );
        cache.clear();
        assert!(
            cache
                .get(&TileCoord {
                    x: 0,
                    y: 0,
                    zoom: 0
                })
                .is_none()
        );
    }

    #[test]
    fn tile_cache_evicts_lru_over_budget() {
        let mut cache = TileCache::with_max_bytes(10);
        let coord = |x| TileCoord { x, y: 0, zoom: 0 };

        cache.insert(coord(0), vec![0u8; 6]);
        cache.insert(coord(1), vec![0u8; 4]);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.byte_len(), 10);

        cache.insert(coord(2), vec![0u8; 5]);
        assert!(cache.byte_len() <= 10);
        assert!(cache.get(&coord(0)).is_none());
        assert!(cache.get(&coord(1)).is_some());
        assert!(cache.get(&coord(2)).is_some());
    }

    #[test]
    fn tile_cache_touch_protects_from_eviction() {
        let mut cache = TileCache::with_max_bytes(10);
        let coord = |x| TileCoord { x, y: 0, zoom: 0 };

        cache.insert(coord(0), vec![0u8; 5]);
        cache.insert(coord(1), vec![0u8; 5]);
        assert!(cache.touch(&coord(0)));

        cache.insert(coord(2), vec![0u8; 5]);
        assert!(cache.get(&coord(0)).is_some());
        assert!(cache.get(&coord(1)).is_none());
        assert!(cache.get(&coord(2)).is_some());
    }

    #[test]
    fn tile_cache_is_bounded_by_default() {
        let mut cache = TileCache::new();
        for x in 0..100 {
            cache.insert(TileCoord { x, y: 0, zoom: 0 }, vec![0u8; 1000]);
        }
        assert_eq!(cache.len(), 100);
        assert_eq!(cache.max_bytes(), TileCache::DEFAULT_MAX_BYTES);
    }

    #[test]
    fn tile_cache_visible_tiles() {
        let cache = TileCache::new();
        let viewport = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 512.0,
            height: 256.0,
        };
        let tiles = cache.visible_tiles(&viewport, 1);
        assert_eq!(tiles.len(), 2);
        assert!(tiles.contains(&TileCoord {
            x: 0,
            y: 0,
            zoom: 1
        }));
        assert!(tiles.contains(&TileCoord {
            x: 1,
            y: 0,
            zoom: 1
        }));
    }

    #[test]
    fn tile_cache_rejects_oversized_replacements_without_flushing() {
        let mut cache = TileCache::with_max_bytes(10);
        let first = TileCoord {
            x: 0,
            y: 0,
            zoom: 0,
        };
        let second = TileCoord {
            x: 1,
            y: 0,
            zoom: 0,
        };
        cache.insert(first, vec![1; 5]);
        cache.insert(second, vec![2; 5]);
        cache.insert(first, vec![3; 11]);
        assert_eq!(cache.get(&first), Some(&vec![1; 5]));
        assert!(cache.get(&second).is_some());
        assert_eq!(cache.byte_len(), 10);
    }

    #[test]
    fn tile_cache_clock_rollover_preserves_lru_order() {
        let mut cache = TileCache::with_max_bytes(2);
        let old = TileCoord {
            x: 0,
            y: 0,
            zoom: 0,
        };
        let recent = TileCoord {
            x: 1,
            y: 0,
            zoom: 0,
        };
        cache.insert(old, vec![0]);
        cache.insert(recent, vec![1]);
        cache.clock = u64::MAX;
        cache.touch(&recent);
        cache.insert(
            TileCoord {
                x: 2,
                y: 0,
                zoom: 0,
            },
            vec![2],
        );
        assert!(cache.get(&old).is_none());
        assert!(cache.get(&recent).is_some());
    }

    #[test]
    fn invalid_or_unbounded_viewports_return_no_tiles() {
        let cache = TileCache::new();
        for viewport in [
            CanvasRect {
                x: f64::NAN,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            CanvasRect {
                x: 0.0,
                y: 0.0,
                width: -1.0,
                height: 10.0,
            },
            CanvasRect {
                x: 0.0,
                y: 0.0,
                width: f64::MAX,
                height: f64::MAX,
            },
        ] {
            assert!(cache.visible_tiles(&viewport, 0).is_empty());
        }
    }

    #[test]
    fn vector_geometry_ignores_non_finite_coordinates() {
        let mut path = VectorPath::new();
        path.add_segment(PathSegment {
            segment_type: PathSegmentType::MoveTo,
            points: vec![pt(f64::NAN, 1.0), pt(2.0, 3.0)],
        });
        assert_eq!(path.bounds().unwrap().x, 2.0);
        path.translate(f64::INFINITY, 1.0);
        assert_eq!(path.segments[0].points[1], pt(2.0, 4.0));
    }

    #[test]
    fn canvas_point_serialization() {
        let p = pt(3.25, 2.72);
        let json = serde_json::to_string(&p).unwrap();
        let deser: CanvasPoint = serde_json::from_str(&json).unwrap();
        assert!((deser.x - 3.25).abs() < f64::EPSILON);
    }
}
