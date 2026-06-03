//! Serializable per-clip geometric transform.
//!
//! [`ClipTransform`] positions and scales a clip's image within the frame — picture-in-
//! picture, Ken Burns, reframing (P2-A "transform"). It maps to the reference compositor's
//! `place` op, so it composes with the effect stack and the compositor like any other
//! clip-applied step.

use kael_render_graph::reference::{place, Image};
use serde::{Deserialize, Serialize};

/// A clip's position and scale within the frame, in normalized coordinates. The clip is
/// drawn into a `scale_x` × `scale_y` box (relative to the frame) centered at
/// `(center_x, center_y)` (`0.5, 0.5` is the frame center); area outside the box is
/// transparent. The identity (`scale 1, center 0.5`) fills the frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipTransform {
    /// Horizontal size relative to the frame width (`1.0` is full width).
    pub scale_x: f32,
    /// Vertical size relative to the frame height (`1.0` is full height).
    pub scale_y: f32,
    /// Normalized horizontal center of the box (`0.5` is centered).
    pub center_x: f32,
    /// Normalized vertical center of the box (`0.5` is centered).
    pub center_y: f32,
}

impl Default for ClipTransform {
    fn default() -> Self {
        Self {
            scale_x: 1.0,
            scale_y: 1.0,
            center_x: 0.5,
            center_y: 0.5,
        }
    }
}

impl ClipTransform {
    /// Whether this transform leaves the image unchanged (fills the frame, centered).
    pub fn is_identity(&self) -> bool {
        self.scale_x == 1.0 && self.scale_y == 1.0 && self.center_x == 0.5 && self.center_y == 0.5
    }

    /// Apply the transform to `source`, returning a same-size image with the clip scaled and
    /// positioned into its box (the rest transparent). The identity returns an unchanged copy.
    pub fn apply(&self, source: &Image) -> Image {
        if self.is_identity() {
            return source.clone();
        }
        let (width, height) = (source.width, source.height);
        let dst_width = (self.scale_x * width as f32).round().max(0.0) as u32;
        let dst_height = (self.scale_y * height as f32).round().max(0.0) as u32;
        let dst_x = (self.center_x * width as f32 - dst_width as f32 / 2.0).round() as i32;
        let dst_y = (self.center_y * height as f32 - dst_height as f32 / 2.0).round() as i32;
        let mut output = Image::new(width, height);
        place(dst_x, dst_y, dst_width, dst_height)(&[source], &mut output);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_transform_identity_passes_through() {
        let mut source = Image::new(4, 4);
        source.pixels[5] = [0.8, 0.2, 0.5, 1.0];
        assert_eq!(
            ClipTransform::default().apply(&source).pixels,
            source.pixels
        );
    }

    #[test]
    fn clip_transform_scales_into_a_centered_box() {
        // Half-size, centered: a white frame becomes a white square over the middle, with
        // transparent borders.
        let source = Image::filled(4, 4, [1.0, 1.0, 1.0, 1.0]);
        let transform = ClipTransform {
            scale_x: 0.5,
            scale_y: 0.5,
            center_x: 0.5,
            center_y: 0.5,
        };
        let out = transform.apply(&source);
        // The destination box is [1,3) x [1,3): center is opaque, corners transparent.
        assert_eq!(out.pixel(2, 2), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(out.pixel(0, 0)[3], 0.0);
        assert_eq!(out.pixel(3, 3)[3], 0.0);
    }

    #[test]
    fn clip_transform_positions_the_box() {
        // A half-size box anchored at the top-left quadrant lands content there, not center.
        let source = Image::filled(4, 4, [1.0, 1.0, 1.0, 1.0]);
        let transform = ClipTransform {
            scale_x: 0.5,
            scale_y: 0.5,
            center_x: 0.25,
            center_y: 0.25,
        };
        let out = transform.apply(&source);
        // Box centered at (1,1) spanning [0,2) x [0,2): top-left opaque, center transparent.
        assert_eq!(out.pixel(0, 0)[3], 1.0);
        assert_eq!(out.pixel(3, 3)[3], 0.0);
    }

    #[test]
    fn clip_transform_serde_round_trips() {
        let transform = ClipTransform {
            scale_x: 0.75,
            scale_y: 0.5,
            center_x: 0.3,
            center_y: 0.6,
        };
        let json = serde_json::to_string(&transform).unwrap();
        assert_eq!(
            serde_json::from_str::<ClipTransform>(&json).unwrap(),
            transform
        );
    }
}
