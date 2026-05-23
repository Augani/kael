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
        let mut points = self.segments.iter().flat_map(|s| s.points.iter());
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
        self.segments.iter().map(|s| s.points.len()).sum()
    }

    /// Translate all points by the given offset.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        for seg in &mut self.segments {
            for p in &mut seg.points {
                p.x += dx;
                p.y += dy;
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
        if self.dpi <= 0.0 {
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

/// Cache for rendered tile data.
#[derive(Debug, Default)]
pub struct TileCache {
    tiles: HashMap<TileCoord, Vec<u8>>,
}

impl TileCache {
    /// Create a new empty tile cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert tile data at the given coordinate.
    pub fn insert(&mut self, coord: TileCoord, data: Vec<u8>) {
        self.tiles.insert(coord, data);
    }

    /// Get cached tile data.
    pub fn get(&self, coord: &TileCoord) -> Option<&Vec<u8>> {
        self.tiles.get(coord)
    }

    /// Remove a single tile from the cache. Returns true if the tile existed.
    pub fn invalidate(&mut self, coord: &TileCoord) -> bool {
        self.tiles.remove(coord).is_some()
    }

    /// Remove all cached tiles.
    pub fn clear(&mut self) {
        self.tiles.clear();
    }

    /// Return the coordinates of tiles visible within a viewport at the given zoom level.
    pub fn visible_tiles(&self, viewport: &CanvasRect, zoom: u8) -> Vec<TileCoord> {
        let tile_size = 256.0;
        let start_x = (viewport.x / tile_size).floor() as i32;
        let start_y = (viewport.y / tile_size).floor() as i32;
        let end_x = ((viewport.x + viewport.width) / tile_size).ceil() as i32;
        let end_y = ((viewport.y + viewport.height) / tile_size).ceil() as i32;

        let mut coords = Vec::new();
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
    fn canvas_point_serialization() {
        let p = pt(3.14, 2.72);
        let json = serde_json::to_string(&p).unwrap();
        let deser: CanvasPoint = serde_json::from_str(&json).unwrap();
        assert!((deser.x - 3.14).abs() < f64::EPSILON);
    }
}
