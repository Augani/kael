//! Serializable per-clip geometric transform.
//!
//! [`ClipTransform`] positions and scales a clip's image within the frame — picture-in-
//! picture, Ken Burns, reframing (P2-A "transform"). It maps to the reference compositor's
//! `place` op, so it composes with the effect stack and the compositor like any other
//! clip-applied step.

use kael_render_graph::reference::{Image, PassOp, place, rotate};
use serde::{Deserialize, Serialize};

use crate::automation::Automation;

/// A clip's geometric transform within the frame, in normalized coordinates: the content is
/// rotated about its center by `rotation` radians, then drawn into a `scale_x` × `scale_y`
/// box (relative to the frame) centered at `(center_x, center_y)` (`0.5, 0.5` is the frame
/// center); area outside the box is transparent. The identity (`scale 1, center 0.5,
/// rotation 0`) fills the frame.
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
    /// Counter-clockwise rotation of the content about its center, in radians.
    #[serde(default)]
    pub rotation: f32,
}

impl Default for ClipTransform {
    fn default() -> Self {
        Self {
            scale_x: 1.0,
            scale_y: 1.0,
            center_x: 0.5,
            center_y: 0.5,
            rotation: 0.0,
        }
    }
}

impl ClipTransform {
    /// Whether this transform leaves the image unchanged (fills the frame, centered, unrotated).
    pub fn is_identity(&self) -> bool {
        self.scale_x == 1.0
            && self.scale_y == 1.0
            && self.center_x == 0.5
            && self.center_y == 0.5
            && self.rotation == 0.0
    }

    fn box_is_identity(&self) -> bool {
        self.scale_x == 1.0 && self.scale_y == 1.0 && self.center_x == 0.5 && self.center_y == 0.5
    }

    fn dst_rect(&self, width: u32, height: u32) -> (i32, i32, u32, u32) {
        let dst_width = (self.scale_x * width as f32).round().max(0.0) as u32;
        let dst_height = (self.scale_y * height as f32).round().max(0.0) as u32;
        let dst_x = (self.center_x * width as f32 - dst_width as f32 / 2.0).round() as i32;
        let dst_y = (self.center_y * height as f32 - dst_height as f32 / 2.0).round() as i32;
        (dst_x, dst_y, dst_width, dst_height)
    }

    /// Apply the transform to `source`: rotate the content about its center, then scale and
    /// position it into its box (the rest transparent). The identity returns an unchanged copy.
    pub fn apply(&self, source: &Image) -> Image {
        if self.is_identity() {
            return source.clone();
        }
        let (width, height) = (source.width, source.height);

        let mut rotated = Image::new(width, height);
        if self.rotation != 0.0 {
            rotate(self.rotation)(&[source], &mut rotated);
        } else {
            rotated.pixels.clone_from(&source.pixels);
        }

        if self.box_is_identity() {
            return rotated;
        }
        let (dst_x, dst_y, dst_width, dst_height) = self.dst_rect(width, height);
        let mut output = Image::new(width, height);
        place(dst_x, dst_y, dst_width, dst_height)(&[&rotated], &mut output);
        output
    }

    /// A single-input render-graph pass that applies this transform to `inputs[0]` — the
    /// graph form of [`apply`](Self::apply), for the compositor's frame graph.
    pub fn to_pass_op(self) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            let transformed = self.apply(source);
            if transformed.pixels.len() == output.pixels.len() {
                output.pixels.clone_from(&transformed.pixels);
            }
        })
    }

    /// A stable hash of the transform's parameters, for render-graph cache keys.
    pub fn param_hash(self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in [
            self.scale_x,
            self.scale_y,
            self.center_x,
            self.center_y,
            self.rotation,
        ] {
            for byte in value.to_bits().to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

/// A [`ClipTransform`] whose parameters are [`Automation`] curves rather than constants — a
/// keyframed transform (Ken Burns / animated PiP). [`resolve`](AnimatedClipTransform::resolve)
/// samples every curve at a time to produce the concrete [`ClipTransform`] for that moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatedClipTransform {
    /// Horizontal scale over time.
    pub scale_x: Automation,
    /// Vertical scale over time.
    pub scale_y: Automation,
    /// Normalized horizontal center over time.
    pub center_x: Automation,
    /// Normalized vertical center over time.
    pub center_y: Automation,
    /// Rotation (radians) over time.
    pub rotation: Automation,
}

impl AnimatedClipTransform {
    /// Sample every parameter curve at `time_ms` to produce the concrete transform.
    pub fn resolve(&self, time_ms: u64) -> ClipTransform {
        ClipTransform {
            scale_x: self.scale_x.sample(time_ms),
            scale_y: self.scale_y.sample(time_ms),
            center_x: self.center_x.sample(time_ms),
            center_y: self.center_y.sample(time_ms),
            rotation: self.rotation.sample(time_ms),
        }
    }
}

impl From<ClipTransform> for AnimatedClipTransform {
    /// Lift a static transform into a keyframed one whose parameters are constant curves.
    fn from(transform: ClipTransform) -> Self {
        Self {
            scale_x: Automation::constant(transform.scale_x),
            scale_y: Automation::constant(transform.scale_y),
            center_x: Automation::constant(transform.center_x),
            center_y: Automation::constant(transform.center_y),
            rotation: Automation::constant(transform.rotation),
        }
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
            rotation: 0.0,
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
            rotation: 0.0,
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
            rotation: 1.2,
        };
        let json = serde_json::to_string(&transform).unwrap();
        assert_eq!(
            serde_json::from_str::<ClipTransform>(&json).unwrap(),
            transform
        );
    }

    #[test]
    fn clip_transform_rotates_the_content() {
        let mut source = Image::filled(3, 3, [0.0, 0.0, 0.0, 1.0]);
        source.pixels[0] = [1.0, 0.0, 0.0, 1.0]; // (0,0) red
        let transform = ClipTransform {
            rotation: std::f32::consts::PI,
            ..ClipTransform::default()
        };
        assert!(!transform.is_identity());
        // 180 deg about center moves the red corner to (2,2).
        let out = transform.apply(&source);
        assert!((out.pixel(2, 2)[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn animated_transform_resolves_keyframed_scale() {
        use crate::automation::{Interpolation, Keyframe};
        let mut scale = Automation::constant(1.0);
        scale.add(Keyframe {
            time_ms: 0,
            value: 1.0,
            interpolation: Interpolation::Linear,
        });
        scale.add(Keyframe {
            time_ms: 1000,
            value: 0.5,
            interpolation: Interpolation::Linear,
        });
        let animated = AnimatedClipTransform {
            scale_x: scale.clone(),
            scale_y: scale,
            center_x: Automation::constant(0.5),
            center_y: Automation::constant(0.5),
            rotation: Automation::constant(0.0),
        };
        assert!((animated.resolve(0).scale_x - 1.0).abs() < 1e-6);
        assert!((animated.resolve(500).scale_x - 0.75).abs() < 1e-6);
        assert!((animated.resolve(1000).scale_x - 0.5).abs() < 1e-6);
        // Unanimated params stay constant.
        assert!((animated.resolve(500).center_x - 0.5).abs() < 1e-6);
    }

    #[test]
    fn animated_transform_from_static_resolves_to_constants() {
        let static_transform = ClipTransform {
            scale_x: 0.5,
            scale_y: 0.5,
            center_x: 0.25,
            center_y: 0.75,
            rotation: 1.0,
        };
        let animated: AnimatedClipTransform = static_transform.into();
        assert_eq!(animated.resolve(0), static_transform);
        assert_eq!(animated.resolve(5000), static_transform);
    }

    #[test]
    fn animated_transform_serde_round_trips() {
        let animated: AnimatedClipTransform = ClipTransform::default().into();
        let json = serde_json::to_string(&animated).unwrap();
        assert_eq!(
            serde_json::from_str::<AnimatedClipTransform>(&json).unwrap(),
            animated
        );
    }
}
