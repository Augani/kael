use crate::{point, size, Bounds, Corners, DevicePixels, Hsla, ScaledPixels, Size};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShadowAtlasParams {
    pub(crate) size: Size<DevicePixels>,
    pub(crate) corner_radii: Corners<ScaledPixels>,
    pub(crate) blur_radius: ScaledPixels,
    pub(crate) color: Hsla,
    pub(crate) inset: bool,
}

impl ShadowAtlasParams {
    pub(crate) fn new(
        size: Size<DevicePixels>,
        corner_radii: Corners<ScaledPixels>,
        blur_radius: ScaledPixels,
        color: Hsla,
        inset: bool,
    ) -> Self {
        Self {
            size,
            corner_radii,
            blur_radius,
            color,
            inset,
        }
    }
}

impl Hash for ShadowAtlasParams {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.size.hash(state);
        self.corner_radii.top_left.0.to_bits().hash(state);
        self.corner_radii.top_right.0.to_bits().hash(state);
        self.corner_radii.bottom_right.0.to_bits().hash(state);
        self.corner_radii.bottom_left.0.to_bits().hash(state);
        self.blur_radius.0.to_bits().hash(state);
        self.color.hash(state);
        self.inset.hash(state);
    }
}

pub(crate) fn expanded_bounds(
    bounds: Bounds<ScaledPixels>,
    blur_radius: ScaledPixels,
) -> Bounds<ScaledPixels> {
    let margin = shadow_margin_pixels(blur_radius) as f32;
    Bounds {
        origin: point(
            ScaledPixels(bounds.origin.x.0 - margin),
            ScaledPixels(bounds.origin.y.0 - margin),
        ),
        size: size(
            ScaledPixels(bounds.size.width.0 + margin * 2.0),
            ScaledPixels(bounds.size.height.0 + margin * 2.0),
        ),
    }
}

pub(crate) fn rasterize_shadow(params: &ShadowAtlasParams) -> (Size<DevicePixels>, Vec<u8>) {
    let margin = shadow_margin_pixels(params.blur_radius);
    let texture_size = size(
        DevicePixels(params.size.width.0 + margin * 2),
        DevicePixels(params.size.height.0 + margin * 2),
    );
    let mut bytes = vec![0; texture_size.width.0 as usize * texture_size.height.0 as usize * 4];
    let color = params.color.to_rgb();
    let bounds = Bounds {
        origin: point(ScaledPixels(margin as f32), ScaledPixels(margin as f32)),
        size: params.size.map(|value| ScaledPixels(value.0 as f32)),
    };

    for y in 0..texture_size.height.0 {
        for x in 0..texture_size.width.0 {
            let point_x = x as f32 + 0.5;
            let point_y = y as f32 + 0.5;
            let mut alpha = shadow_alpha(point_x, point_y, &bounds, params);
            if params.inset {
                alpha = 1.0 - alpha;
            }

            let alpha = (color.a * alpha).clamp(0.0, 1.0);
            let pixel = ((y as usize * texture_size.width.0 as usize) + x as usize) * 4;
            bytes[pixel] = channel_to_byte(color.b * alpha);
            bytes[pixel + 1] = channel_to_byte(color.g * alpha);
            bytes[pixel + 2] = channel_to_byte(color.r * alpha);
            bytes[pixel + 3] = channel_to_byte(alpha);
        }
    }

    (texture_size, bytes)
}

fn shadow_alpha(
    point_x: f32,
    point_y: f32,
    bounds: &Bounds<ScaledPixels>,
    params: &ShadowAtlasParams,
) -> f32 {
    if params.blur_radius.0 <= 0.0 {
        return saturate(0.5 - quad_sdf(point_x, point_y, bounds, &params.corner_radii));
    }

    let origin_x = bounds.origin.x.0;
    let origin_y = bounds.origin.y.0;
    let width = bounds.size.width.0;
    let height = bounds.size.height.0;
    let half_width = width / 2.0;
    let half_height = height / 2.0;
    let center_x = origin_x + half_width;
    let center_y = origin_y + half_height;
    let local_x = point_x - center_x;
    let local_y = point_y - center_y;
    let corner_radius = pick_corner_radius(local_x, local_y, &params.corner_radii);
    let low = local_y - half_height;
    let high = local_y + half_height;
    let start = (-3.0 * params.blur_radius.0).clamp(low, high);
    let end = (3.0 * params.blur_radius.0).clamp(low, high);
    let step = (end - start) / 4.0;
    let mut y = start + step * 0.5;
    let mut alpha = 0.0;

    for _ in 0..4 {
        alpha += blur_along_x(
            local_x,
            local_y - y,
            params.blur_radius.0,
            corner_radius,
            half_width,
            half_height,
        ) * gaussian(y, params.blur_radius.0)
            * step;
        y += step;
    }

    alpha.clamp(0.0, 1.0)
}

fn pick_corner_radius(x: f32, y: f32, corner_radii: &Corners<ScaledPixels>) -> f32 {
    if x < 0.0 {
        if y < 0.0 {
            corner_radii.top_left.0
        } else {
            corner_radii.bottom_left.0
        }
    } else if y < 0.0 {
        corner_radii.top_right.0
    } else {
        corner_radii.bottom_right.0
    }
}

fn quad_sdf(
    point_x: f32,
    point_y: f32,
    bounds: &Bounds<ScaledPixels>,
    corner_radii: &Corners<ScaledPixels>,
) -> f32 {
    let half_width = bounds.size.width.0 / 2.0;
    let half_height = bounds.size.height.0 / 2.0;
    let center_x = bounds.origin.x.0 + half_width;
    let center_y = bounds.origin.y.0 + half_height;
    let center_to_point_x = point_x - center_x;
    let center_to_point_y = point_y - center_y;
    let corner_radius = pick_corner_radius(center_to_point_x, center_to_point_y, corner_radii);
    let corner_to_point_x = center_to_point_x.abs() - half_width;
    let corner_to_point_y = center_to_point_y.abs() - half_height;
    quad_sdf_impl(
        corner_to_point_x + corner_radius,
        corner_to_point_y + corner_radius,
        corner_radius,
    )
}

fn quad_sdf_impl(corner_x: f32, corner_y: f32, corner_radius: f32) -> f32 {
    if corner_radius == 0.0 {
        corner_x.max(corner_y)
    } else {
        let inset = corner_x.max(corner_y).min(0.0)
            + (corner_x.max(0.0).powi(2) + corner_y.max(0.0).powi(2)).sqrt();
        inset - corner_radius
    }
}

fn blur_along_x(
    x: f32,
    y: f32,
    sigma: f32,
    corner_radius: f32,
    half_width: f32,
    half_height: f32,
) -> f32 {
    let delta = (half_height - corner_radius - y.abs()).min(0.0);
    let curved = half_width - corner_radius
        + (corner_radius * corner_radius - delta * delta)
            .max(0.0)
            .sqrt();
    let scale = (0.5f32).sqrt() / sigma;
    let left = 0.5 + 0.5 * erf((x - curved) * scale);
    let right = 0.5 + 0.5 * erf((x + curved) * scale);
    right - left
}

fn erf(x: f32) -> f32 {
    let sign = x.signum();
    let value = x.abs();
    let r1 = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * value) * value) * value) * value;
    let r2 = r1 * r1;
    sign - sign / (r2 * r2)
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    (-(x * x) / (2.0 * sigma * sigma)).exp() / ((2.0 * std::f32::consts::PI).sqrt() * sigma)
}

fn shadow_margin_pixels(blur_radius: ScaledPixels) -> i32 {
    (blur_radius.0 * 3.0).ceil().max(0.0) as i32
}

fn saturate(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn channel_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_bounds_add_blur_margin() {
        let expanded = expanded_bounds(
            Bounds {
                origin: point(ScaledPixels(10.0), ScaledPixels(20.0)),
                size: size(ScaledPixels(30.0), ScaledPixels(40.0)),
            },
            ScaledPixels(2.0),
        );

        assert_eq!(
            expanded.origin,
            point(ScaledPixels(4.0), ScaledPixels(14.0))
        );
        assert_eq!(expanded.size, size(ScaledPixels(42.0), ScaledPixels(52.0)));
    }

    #[test]
    fn rasterize_shadow_returns_bgra_bytes() {
        let params = ShadowAtlasParams::new(
            size(DevicePixels(12), DevicePixels(8)),
            Corners::default(),
            ScaledPixels(0.0),
            Hsla::black().opacity(0.5),
            false,
        );

        let (texture_size, bytes) = rasterize_shadow(&params);

        assert_eq!(texture_size, crate::size(DevicePixels(12), DevicePixels(8)));
        assert_eq!(bytes.len(), 12 * 8 * 4);
    }
}
