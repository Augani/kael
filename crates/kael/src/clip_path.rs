//! Arbitrary clip-shape geometry: the device-independent math for clipping content
//! to non-rectangular shapes (circle, ellipse, convex polygon).
//!
//! This is the shape model + coverage math shared by the two consumers a full
//! clip-path subsystem needs: mask rasterization (which samples [`ClipShape::coverage`]
//! to build an alpha mask via [`ClipShape::rasterize_mask`]) and clip-aware hit-testing
//! (which calls [`ClipShape::contains`] so input outside the visible shape does not hit).
//!
//! Clipping rendered output to an arbitrary shape works end-to-end and is golden-verified.
//! Circles clip *live* through the existing shader: [`ClipShape::as_rounded_clip`] maps a
//! circle to the rounded-rect clip the quad shader already honors, surfaced as
//! [`crate::Window::with_clip_path`] (see `circle_clip_shape_renders_through_the_rounded_clip_shader`).
//! Convex polygons clip correctly via a rasterized mask + [`apply_clip_mask_bgra`] (see
//! `arbitrary_triangle_clip_produces_correct_pixels`); `with_clip_path` falls back to the
//! shape's bounding box for them until the per-texel mask sample is fused into the in-pass
//! shader. A `Styled::clip_path()` builder is the remaining sugar.

use crate::{point, px, size, Bounds, Corners, Pixels, Point, Size};

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

    /// Rasterize this shape's anti-aliased coverage into a `width`×`height` row-major
    /// alpha mask (one `f32` in `[0.0, 1.0]` per texel), sampling at pixel centers in the
    /// coordinate space whose top-left texel center is at `origin + (0.5, 0.5)`. This is
    /// the CPU reference a GPU mask pass must match, and the input to [`apply_clip_mask_bgra`].
    pub fn rasterize_mask(
        &self,
        origin: Point<Pixels>,
        width: usize,
        height: usize,
        aa_width: Pixels,
    ) -> Vec<f32> {
        let mut mask = vec![0.0f32; width.saturating_mul(height)];
        for y in 0..height {
            for x in 0..width {
                let sample = point(
                    px(origin.x.0 + x as f32 + 0.5),
                    px(origin.y.0 + y as f32 + 0.5),
                );
                mask[y * width + x] = self.coverage(sample, aa_width);
            }
        }
        mask
    }

    /// Express this shape as the equivalent axis-aligned rounded-rectangle clip
    /// (`bounds`, corner `radii`) when it can be represented exactly that way — a circle,
    /// or an ellipse whose radii are equal. Returns `None` for shapes the rounded-rect clip
    /// cannot express exactly (true ellipses, convex polygons), which need the mask path.
    ///
    /// This lets circle clips reuse the framework's existing shader-backed rounded-clip
    /// pipeline ([`crate::Window::with_rounded_clip`]) with no new GPU code.
    pub fn as_rounded_clip(&self) -> Option<(Bounds<Pixels>, Corners<Pixels>)> {
        let circle = |center: Point<Pixels>, radius: Pixels| {
            (
                Bounds {
                    origin: point(center.x - radius, center.y - radius),
                    size: size(radius * 2.0, radius * 2.0),
                },
                Corners::all(radius),
            )
        };
        match self {
            ClipShape::Circle { center, radius } => Some(circle(*center, *radius)),
            ClipShape::Ellipse { center, radii } => {
                if (radii.width.0 - radii.height.0).abs() < f32::EPSILON {
                    Some(circle(*center, radii.width))
                } else {
                    None
                }
            }
            ClipShape::ConvexPolygon { .. } => None,
        }
    }
}

/// Apply a coverage `mask` to a tightly-packed 8-bit BGRA (or RGBA) pixel buffer in place,
/// scaling each pixel's alpha by its mask value. The visual effect is a clip: content is
/// kept where the mask is `1.0` and cut where it is `0.0`, with anti-aliased edges in
/// between. `mask` is row-major with one entry per pixel; extra pixels are left untouched.
pub fn apply_clip_mask_bgra(pixels: &mut [u8], mask: &[f32]) {
    for (index, &coverage) in mask.iter().enumerate() {
        let alpha_byte = index * 4 + 3;
        if alpha_byte >= pixels.len() {
            break;
        }
        let scaled = pixels[alpha_byte] as f32 * coverage.clamp(0.0, 1.0);
        pixels[alpha_byte] = scaled.round().clamp(0.0, 255.0) as u8;
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

    #[test]
    fn rasterize_mask_is_opaque_inside_and_clear_outside() {
        let circle = ClipShape::Circle {
            center: pt(16.0, 16.0),
            radius: px(12.0),
        };
        let mask = circle.rasterize_mask(point(px(0.0), px(0.0)), 32, 32, px(1.0));
        assert_eq!(mask.len(), 32 * 32);

        let center = mask[16 * 32 + 16];
        let corner = mask[0];
        assert!(center > 0.99, "center texel is fully covered, got {center}");
        assert!(corner < 0.01, "corner texel is uncovered, got {corner}");
    }

    #[test]
    fn apply_clip_mask_scales_alpha() {
        // 2x1 opaque white BGRA pixels; mask keeps the first, cuts the second.
        let mut pixels = vec![255u8, 255, 255, 255, 255, 255, 255, 255];
        apply_clip_mask_bgra(&mut pixels, &[1.0, 0.0]);
        assert_eq!(pixels[3], 255, "kept pixel stays opaque");
        assert_eq!(pixels[7], 0, "cut pixel becomes transparent");

        let mut half = vec![255u8, 255, 255, 200];
        apply_clip_mask_bgra(&mut half, &[0.5]);
        assert_eq!(half[3], 100, "half coverage halves alpha");
    }

    #[test]
    fn triangle_clip_keeps_interior_pixels_and_cuts_exterior() {
        // A full 24x24 opaque buffer clipped to a triangle: a point inside the triangle
        // stays opaque, a corner outside it is cut to transparent.
        let triangle = ClipShape::ConvexPolygon {
            vertices: vec![pt(0.0, 0.0), pt(23.0, 0.0), pt(12.0, 23.0)],
        };
        let mask = triangle.rasterize_mask(point(px(0.0), px(0.0)), 24, 24, px(1.0));
        let mut pixels = vec![255u8; 24 * 24 * 4];
        apply_clip_mask_bgra(&mut pixels, &mask);

        let alpha_at = |x: usize, y: usize| pixels[(y * 24 + x) * 4 + 3];
        assert_eq!(alpha_at(12, 4), 255, "near the apex, inside, stays opaque");
        assert_eq!(alpha_at(1, 22), 0, "bottom-left corner is outside, cut");
        assert_eq!(alpha_at(22, 22), 0, "bottom-right corner is outside, cut");
    }

    #[test]
    fn circle_maps_to_an_equivalent_rounded_rect_clip() {
        let circle = ClipShape::Circle {
            center: pt(50.0, 50.0),
            radius: px(20.0),
        };
        let (bounds, radii) = circle.as_rounded_clip().expect("a circle maps exactly");
        assert_eq!(bounds.origin, pt(30.0, 30.0));
        assert_eq!(bounds.size, size(px(40.0), px(40.0)));
        assert_eq!(radii.top_left, px(20.0));
        assert_eq!(radii.bottom_right, px(20.0));
    }

    #[test]
    fn equal_radius_ellipse_maps_but_true_ellipse_and_polygon_do_not() {
        let round_ellipse = ClipShape::Ellipse {
            center: pt(0.0, 0.0),
            radii: size(px(15.0), px(15.0)),
        };
        assert!(round_ellipse.as_rounded_clip().is_some());

        let true_ellipse = ClipShape::Ellipse {
            center: pt(0.0, 0.0),
            radii: size(px(40.0), px(10.0)),
        };
        assert!(true_ellipse.as_rounded_clip().is_none());

        let polygon = ClipShape::ConvexPolygon {
            vertices: vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 10.0)],
        };
        assert!(polygon.as_rounded_clip().is_none());
    }
}
