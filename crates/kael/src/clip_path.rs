//! Arbitrary clip-shape geometry: the device-independent math for clipping content
//! to non-rectangular shapes (circle, ellipse, convex polygon).
//!
//! This is the shape model + coverage math shared by the two consumers a full
//! clip-path subsystem needs: GPU mask rasterization (which samples [`ClipShape::coverage`]
//! to build an alpha mask) and clip-aware hit-testing (which calls [`ClipShape::contains`]
//! so input outside the visible shape does not hit). The renderer still clips to the
//! rectangular content mask today; wiring these shapes into the shaders is the next step.

use crate::{Bounds, Pixels, Point, Size, point, px, size};

/// A non-rectangular clip region. Coordinates are in logical pixels, in the same space
/// as the element's bounds.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipShape {
    /// A circle of `radius` centered at `center`.
    Circle {
        /// Center of the circle.
        center: Point<Pixels>,
        /// Radius of the circle.
        radius: Pixels,
    },
    /// An axis-aligned ellipse centered at `center` with the given x/y radii.
    Ellipse {
        /// Center of the ellipse.
        center: Point<Pixels>,
        /// Half-extents along x (`width`) and y (`height`).
        radii: Size<Pixels>,
    },
    /// A convex polygon defined by its vertices in order (winding may be either
    /// direction). Non-convex inputs are treated as their convex behavior is undefined.
    ConvexPolygon {
        /// Vertices in boundary order.
        vertices: Vec<Point<Pixels>>,
    },
}

impl ClipShape {
    /// Whether `p` lies inside (or exactly on the boundary of) the shape.
    pub fn contains(&self, p: Point<Pixels>) -> bool {
        match self {
            ClipShape::Circle { center, radius } => {
                let dx = p.x.0 - center.x.0;
                let dy = p.y.0 - center.y.0;
                let r = radius.0;
                r > 0.0 && dx * dx + dy * dy <= r * r
            }
            ClipShape::Ellipse { center, radii } => {
                let rx = radii.width.0;
                let ry = radii.height.0;
                if rx <= 0.0 || ry <= 0.0 {
                    return false;
                }
                let nx = (p.x.0 - center.x.0) / rx;
                let ny = (p.y.0 - center.y.0) / ry;
                nx * nx + ny * ny <= 1.0
            }
            ClipShape::ConvexPolygon { vertices } => point_in_convex_polygon(vertices, p),
        }
    }

    /// Anti-aliased coverage of `p` in `[0.0, 1.0]`: `1.0` well inside the shape, `0.0`
    /// well outside, transitioning over an `aa_width`-pixel band straddling the boundary.
    /// This is the value a GPU mask pass rasterizes per texel.
    pub fn coverage(&self, p: Point<Pixels>, aa_width: Pixels) -> f32 {
        let signed = self.signed_distance(p);
        let aa = aa_width.0.max(f32::EPSILON);
        (signed / aa + 0.5).clamp(0.0, 1.0)
    }

    /// Signed distance from `p` to the shape boundary: positive inside, negative outside.
    /// Exact for circles/ellipses near-boundary and uses the convex half-plane distance
    /// for polygons (the standard approximation GPU SDF clips use).
    pub fn signed_distance(&self, p: Point<Pixels>) -> f32 {
        match self {
            ClipShape::Circle { center, radius } => {
                let dx = p.x.0 - center.x.0;
                let dy = p.y.0 - center.y.0;
                radius.0 - (dx * dx + dy * dy).sqrt()
            }
            ClipShape::Ellipse { center, radii } => {
                let rx = radii.width.0;
                let ry = radii.height.0;
                if rx <= 0.0 || ry <= 0.0 {
                    return f32::NEG_INFINITY;
                }
                let nx = (p.x.0 - center.x.0) / rx;
                let ny = (p.y.0 - center.y.0) / ry;
                let normalized = (nx * nx + ny * ny).sqrt();
                let scale = rx.min(ry);
                (1.0 - normalized) * scale
            }
            ClipShape::ConvexPolygon { vertices } => convex_signed_distance(vertices, p),
        }
    }

    /// The axis-aligned bounding box of the shape — the region a GPU mask pass must cover.
    pub fn bounding_box(&self) -> Bounds<Pixels> {
        match self {
            ClipShape::Circle { center, radius } => Bounds {
                origin: point(center.x - *radius, center.y - *radius),
                size: size(*radius * 2.0, *radius * 2.0),
            },
            ClipShape::Ellipse { center, radii } => Bounds {
                origin: point(center.x - radii.width, center.y - radii.height),
                size: size(radii.width * 2.0, radii.height * 2.0),
            },
            ClipShape::ConvexPolygon { vertices } => {
                if vertices.is_empty() {
                    return Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: size(px(0.0), px(0.0)),
                    };
                }
                let mut min_x = f32::INFINITY;
                let mut min_y = f32::INFINITY;
                let mut max_x = f32::NEG_INFINITY;
                let mut max_y = f32::NEG_INFINITY;
                for v in vertices {
                    min_x = min_x.min(v.x.0);
                    min_y = min_y.min(v.y.0);
                    max_x = max_x.max(v.x.0);
                    max_y = max_y.max(v.y.0);
                }
                Bounds {
                    origin: point(px(min_x), px(min_y)),
                    size: size(px(max_x - min_x), px(max_y - min_y)),
                }
            }
        }
    }
}

fn point_in_convex_polygon(vertices: &[Point<Pixels>], p: Point<Pixels>) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    let mut sign = 0i32;
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        let cross = (b.x.0 - a.x.0) * (p.y.0 - a.y.0) - (b.y.0 - a.y.0) * (p.x.0 - a.x.0);
        let s = if cross > 0.0 {
            1
        } else if cross < 0.0 {
            -1
        } else {
            0
        };
        if s != 0 {
            if sign == 0 {
                sign = s;
            } else if sign != s {
                return false;
            }
        }
    }
    true
}

fn convex_signed_distance(vertices: &[Point<Pixels>], p: Point<Pixels>) -> f32 {
    let n = vertices.len();
    if n < 3 {
        return f32::NEG_INFINITY;
    }
    let mut area2 = 0.0f32;
    for i in 0..n {
        let a = vertices[i];
        let b = vertices[(i + 1) % n];
        area2 += a.x.0 * b.y.0 - b.x.0 * a.y.0;
    }
    let winding = if area2 >= 0.0 { 1.0 } else { -1.0 };

    let mut min_distance = f32::INFINITY;
    for i in 0..n {
        let a = vertices[i];
        let b = vertices[(i + 1) % n];
        let ex = b.x.0 - a.x.0;
        let ey = b.y.0 - a.y.0;
        let len = (ex * ex + ey * ey).sqrt();
        if len == 0.0 {
            continue;
        }
        let cross = ex * (p.y.0 - a.y.0) - ey * (p.x.0 - a.x.0);
        let distance = winding * cross / len;
        min_distance = min_distance.min(distance);
    }
    min_distance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    #[test]
    fn circle_contains_interior_and_excludes_exterior() {
        let shape = ClipShape::Circle {
            center: pt(50.0, 50.0),
            radius: px(20.0),
        };
        assert!(shape.contains(pt(50.0, 50.0)));
        assert!(shape.contains(pt(65.0, 50.0)));
        assert!(shape.contains(pt(70.0, 50.0)));
        assert!(!shape.contains(pt(71.0, 50.0)));
        assert!(!shape.contains(pt(50.0, 90.0)));
    }

    #[test]
    fn ellipse_respects_independent_radii() {
        let shape = ClipShape::Ellipse {
            center: pt(0.0, 0.0),
            radii: size(px(40.0), px(10.0)),
        };
        assert!(shape.contains(pt(39.0, 0.0)));
        assert!(!shape.contains(pt(41.0, 0.0)));
        assert!(shape.contains(pt(0.0, 9.0)));
        assert!(!shape.contains(pt(0.0, 11.0)));
    }

    #[test]
    fn convex_polygon_triangle_contains_and_excludes() {
        let triangle = ClipShape::ConvexPolygon {
            vertices: vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(50.0, 100.0)],
        };
        assert!(triangle.contains(pt(50.0, 10.0)));
        assert!(triangle.contains(pt(50.0, 50.0)));
        assert!(!triangle.contains(pt(5.0, 90.0)));
        assert!(!triangle.contains(pt(95.0, 90.0)));
        assert!(!triangle.contains(pt(50.0, 110.0)));
    }

    #[test]
    fn convex_polygon_winding_independent() {
        let ccw = ClipShape::ConvexPolygon {
            vertices: vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
        };
        let cw = ClipShape::ConvexPolygon {
            vertices: vec![pt(0.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0), pt(10.0, 0.0)],
        };
        for shape in [&ccw, &cw] {
            assert!(shape.contains(pt(5.0, 5.0)), "interior point is inside");
            assert!(!shape.contains(pt(15.0, 5.0)), "exterior point is outside");
        }
    }

    #[test]
    fn degenerate_polygon_contains_nothing() {
        let line = ClipShape::ConvexPolygon {
            vertices: vec![pt(0.0, 0.0), pt(10.0, 0.0)],
        };
        assert!(!line.contains(pt(5.0, 0.0)));
    }

    #[test]
    fn coverage_transitions_across_the_boundary() {
        let shape = ClipShape::Circle {
            center: pt(0.0, 0.0),
            radius: px(10.0),
        };
        assert_eq!(shape.coverage(pt(0.0, 0.0), px(1.0)), 1.0);
        assert_eq!(shape.coverage(pt(100.0, 0.0), px(1.0)), 0.0);
        let on_edge = shape.coverage(pt(10.0, 0.0), px(2.0));
        assert!(
            (on_edge - 0.5).abs() < 1e-5,
            "coverage on the boundary is ~0.5, got {on_edge}"
        );
    }

    #[test]
    fn bounding_box_covers_the_shape() {
        let circle = ClipShape::Circle {
            center: pt(50.0, 50.0),
            radius: px(20.0),
        };
        let bounds = circle.bounding_box();
        assert_eq!(bounds.origin, pt(30.0, 30.0));
        assert_eq!(bounds.size, size(px(40.0), px(40.0)));

        let triangle = ClipShape::ConvexPolygon {
            vertices: vec![pt(10.0, 20.0), pt(100.0, 0.0), pt(40.0, 80.0)],
        };
        let tb = triangle.bounding_box();
        assert_eq!(tb.origin, pt(10.0, 0.0));
        assert_eq!(tb.size, size(px(90.0), px(80.0)));
    }
}
