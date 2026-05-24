use crate::{Bounds, Point, ScaledPixels, Size};

/// Consistent pixel-snapping helper for fills, strokes, clips, and text baselines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelSnapPolicy {
    scale: f32,
    inv_scale: f32,
}

impl PixelSnapPolicy {
    /// Create a new policy for the given display scale factor.
    pub fn new(scale_factor: f32) -> Self {
        Self {
            scale: scale_factor,
            inv_scale: 1.0 / scale_factor,
        }
    }

    /// Snap a point to the nearest device pixel boundary.
    pub fn snap_point(&self, point: Point<ScaledPixels>) -> Point<ScaledPixels> {
        Point {
            x: ScaledPixels(self.snap_value(point.x.0)),
            y: ScaledPixels(self.snap_value(point.y.0)),
        }
    }

    /// Expand bounds outward to the nearest device pixel boundaries.
    pub fn snap_bounds_outward(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        let min_x = self.snap_floor(bounds.origin.x.0);
        let min_y = self.snap_floor(bounds.origin.y.0);
        let max_x = self.snap_ceil(bounds.origin.x.0 + bounds.size.width.0);
        let max_y = self.snap_ceil(bounds.origin.y.0 + bounds.size.height.0);
        Bounds {
            origin: Point {
                x: ScaledPixels(min_x),
                y: ScaledPixels(min_y),
            },
            size: Size {
                width: ScaledPixels(max_x - min_x),
                height: ScaledPixels(max_y - min_y),
            },
        }
    }

    /// Contract bounds inward to the nearest device pixel boundaries.
    pub fn snap_bounds_inward(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        let min_x = self.snap_ceil(bounds.origin.x.0);
        let min_y = self.snap_ceil(bounds.origin.y.0);
        let max_x = self.snap_floor(bounds.origin.x.0 + bounds.size.width.0);
        let max_y = self.snap_floor(bounds.origin.y.0 + bounds.size.height.0);
        let width = (max_x - min_x).max(0.0);
        let height = (max_y - min_y).max(0.0);
        Bounds {
            origin: Point {
                x: ScaledPixels(min_x),
                y: ScaledPixels(min_y),
            },
            size: Size {
                width: ScaledPixels(width),
                height: ScaledPixels(height),
            },
        }
    }

    /// Align a stroke center so it lands on crisp device pixel boundaries.
    pub fn snap_stroke_center(&self, position: f32, stroke_width: f32) -> f32 {
        let device_stroke = stroke_width * self.scale;
        if (device_stroke - device_stroke.round()).abs() < 0.01 {
            let device_pos = position * self.scale;
            if device_stroke.round() as i32 % 2 == 1 {
                (device_pos.round() + 0.5) * self.inv_scale
            } else {
                device_pos.round() * self.inv_scale
            }
        } else {
            position
        }
    }

    /// Snap a corner radius to device pixel granularity.
    pub fn snap_radius(&self, radius: f32) -> f32 {
        (radius * self.scale).round() * self.inv_scale
    }

    /// Snap a text baseline y-coordinate to the nearest device pixel.
    pub fn snap_baseline(&self, y: f32) -> f32 {
        (y * self.scale).round() * self.inv_scale
    }

    fn snap_value(&self, value: f32) -> f32 {
        (value * self.scale).round() * self.inv_scale
    }

    fn snap_floor(&self, value: f32) -> f32 {
        (value * self.scale).floor() * self.inv_scale
    }

    fn snap_ceil(&self, value: f32) -> f32 {
        (value * self.scale).ceil() * self.inv_scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_at_2x() {
        let policy = PixelSnapPolicy::new(2.0);
        assert_eq!(policy.snap_value(10.3), 10.5);
        assert_eq!(policy.snap_value(10.7), 10.5);
        assert_eq!(policy.snap_value(10.8), 11.0);
    }

    #[test]
    fn snap_at_1x() {
        let policy = PixelSnapPolicy::new(1.0);
        assert_eq!(policy.snap_value(10.3), 10.0);
        assert_eq!(policy.snap_value(10.7), 11.0);
    }

    #[test]
    fn snap_bounds_outward_at_2x() {
        let policy = PixelSnapPolicy::new(2.0);
        let bounds = Bounds {
            origin: Point {
                x: ScaledPixels(10.3),
                y: ScaledPixels(20.7),
            },
            size: Size {
                width: ScaledPixels(50.2),
                height: ScaledPixels(30.1),
            },
        };
        let snapped = policy.snap_bounds_outward(bounds);
        assert_eq!(snapped.origin.x.0, 10.0);
        assert_eq!(snapped.origin.y.0, 20.5);
        assert_eq!((snapped.origin.x.0 + snapped.size.width.0), 60.5);
        assert_eq!((snapped.origin.y.0 + snapped.size.height.0), 51.0);
    }

    #[test]
    fn snap_stroke_odd_at_2x() {
        let policy = PixelSnapPolicy::new(2.0);
        let centered = policy.snap_stroke_center(10.3, 0.5);
        let device_pos = centered * 2.0;
        assert!(
            (device_pos - device_pos.floor() - 0.5).abs() < 0.01
                || (device_pos - device_pos.round()).abs() < 0.01
        );
    }

    #[test]
    fn snap_radius_at_2x() {
        let policy = PixelSnapPolicy::new(2.0);
        assert_eq!(policy.snap_radius(3.3), 3.5);
        assert_eq!(policy.snap_radius(3.7), 3.5);
        assert_eq!(policy.snap_radius(3.8), 4.0);
    }

    #[test]
    fn snap_baseline_at_2x() {
        let policy = PixelSnapPolicy::new(2.0);
        assert_eq!(policy.snap_baseline(15.3), 15.5);
        assert_eq!(policy.snap_baseline(15.0), 15.0);
    }
}
