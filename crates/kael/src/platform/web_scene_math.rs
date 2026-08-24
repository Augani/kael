#[cfg(test)]
use crate::{Bounds, ScaledPixels, TransformationMatrix};
use crate::{Hsla, Rgba};
use std::ops::Range;

const REFRESH_SAMPLE_COUNT: usize = 12;

/// Allocation-free robust estimator for a browser's animation-frame cadence.
///
/// Browsers do not expose monitor refresh rate synchronously. Kael therefore
/// learns it from consecutive `requestAnimationFrame` timestamps and reports
/// the median of a short window, which ignores isolated dropped frames without
/// assuming every display is 60 Hz.
#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct RefreshRateEstimator {
    previous_timestamp_ms: Option<f64>,
    samples_ms: [f32; REFRESH_SAMPLE_COUNT],
    sample_count: usize,
    next_sample: usize,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl Default for RefreshRateEstimator {
    fn default() -> Self {
        Self {
            previous_timestamp_ms: None,
            samples_ms: [0.0; REFRESH_SAMPLE_COUNT],
            sample_count: 0,
            next_sample: 0,
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
impl RefreshRateEstimator {
    /// Record one DOMHighResTimeStamp supplied by `requestAnimationFrame`.
    pub(crate) fn record(&mut self, timestamp_ms: f64) {
        if !timestamp_ms.is_finite() {
            return;
        }
        let previous = self.previous_timestamp_ms.replace(timestamp_ms);
        let Some(delta_ms) = previous.map(|previous| timestamp_ms - previous) else {
            return;
        };

        // Ignore idle/background gaps and impossible clock movement. The lower
        // bound still permits future 360 Hz panels with normal timestamp noise.
        if !(2.0..=50.0).contains(&delta_ms) {
            return;
        }
        self.samples_ms[self.next_sample] = delta_ms as f32;
        self.next_sample = (self.next_sample + 1) % REFRESH_SAMPLE_COUNT;
        self.sample_count = self
            .sample_count
            .saturating_add(1)
            .min(REFRESH_SAMPLE_COUNT);
    }

    /// Estimated display cadence after at least three valid consecutive samples.
    pub(crate) fn refresh_rate_hz(self) -> Option<f32> {
        if self.sample_count < 3 {
            return None;
        }
        let mut samples = self.samples_ms;
        let active = &mut samples[..self.sample_count];
        active.sort_unstable_by(f32::total_cmp);
        let middle = self.sample_count / 2;
        let median_ms = if self.sample_count.is_multiple_of(2) {
            (active[middle - 1] + active[middle]) * 0.5
        } else {
            active[middle]
        };
        let hz = 1_000.0 / median_ms;
        hz.is_finite().then_some(hz.clamp(20.0, 500.0))
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PixelSampleStats {
    pub(crate) hash: u64,
    pub(crate) changed_pixels: usize,
    pub(crate) nontransparent_pixels: usize,
    pub(crate) minimum_luma: u8,
    pub(crate) maximum_luma: u8,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl PixelSampleStats {
    pub(crate) fn luma_range(self) -> u8 {
        self.maximum_luma.saturating_sub(self.minimum_luma)
    }

    pub(crate) fn is_visually_varied(self, pixel_count: usize) -> bool {
        pixel_count > 0
            && self.nontransparent_pixels == pixel_count
            && self.changed_pixels >= pixel_count.div_ceil(100).max(64)
            && self.luma_range() >= 8
    }
}

/// Summarize an RGBA framebuffer sample without retaining or serializing its pixels.
///
/// This is used only by the opt-in browser release smoke. It proves that WebGL
/// produced a varied, opaque frame instead of trusting a frame-presented marker.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn analyze_rgba_sample(bytes: &[u8]) -> Option<PixelSampleStats> {
    let mut pixels = bytes.chunks_exact(4);
    let first = pixels.next()?;
    if !pixels.remainder().is_empty() {
        return None;
    }

    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    let first_pixel = [first[0], first[1], first[2], first[3]];
    let mut changed_pixels = 0usize;
    let mut nontransparent_pixels = usize::from(first[3] == u8::MAX);
    let first_luma = rgb_luma(first[0], first[1], first[2]);
    let mut minimum_luma = first_luma;
    let mut maximum_luma = first_luma;
    for pixel in pixels {
        changed_pixels += usize::from(pixel != first_pixel);
        nontransparent_pixels += usize::from(pixel[3] == u8::MAX);
        let luma = rgb_luma(pixel[0], pixel[1], pixel[2]);
        minimum_luma = minimum_luma.min(luma);
        maximum_luma = maximum_luma.max(luma);
    }

    Some(PixelSampleStats {
        hash,
        changed_pixels,
        nontransparent_pixels,
        minimum_luma,
        maximum_luma,
    })
}

/// Count byte-for-byte differences between two framebuffer samples.
///
/// A size change is treated as a complete mismatch because the samples no
/// longer describe the same framebuffer region.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn differing_sample_bytes(left: &[u8], right: &[u8]) -> usize {
    if left.len() != right.len() {
        return left.len().max(right.len());
    }
    left.iter()
        .zip(right)
        .filter(|(left, right)| left != right)
        .count()
}

#[cfg(any(test, target_arch = "wasm32"))]
fn rgb_luma(red: u8, green: u8, blue: u8) -> u8 {
    let weighted = u32::from(red) * 77 + u32::from(green) * 150 + u32::from(blue) * 29;
    (weighted >> 8) as u8
}

pub(crate) fn rgba_components(color: Hsla) -> [f32; 4] {
    let Rgba { r, g, b, a } = color.to_rgb();
    [r, g, b, a]
}

/// Convert a top-left-origin retained-scene damage rectangle into WebGL's
/// bottom-left-origin integer scissor coordinates.
///
/// The rectangle is expanded by two device pixels before clipping. This keeps
/// antialiasing and fractional primitive edges inside the repaint while still
/// making routine localized updates substantially cheaper than a full frame.
/// Invalid, empty, or unrepresentable input returns `None`; callers must treat
/// that as a request for a conservative full repaint.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn damage_scissor(
    [x, y, width, height]: [f32; 4],
    [viewport_width, viewport_height]: [f32; 2],
) -> Option<[i32; 4]> {
    const AA_GUARD_PIXELS: f32 = 2.0;
    let values = [x, y, width, height, viewport_width, viewport_height];
    if values.iter().any(|value| !value.is_finite())
        || width < 0.0
        || height < 0.0
        || viewport_width <= 0.0
        || viewport_height <= 0.0
        || viewport_width > i32::MAX as f32
        || viewport_height > i32::MAX as f32
    {
        return None;
    }

    let right = x + width;
    let bottom = y + height;
    if !right.is_finite() || !bottom.is_finite() {
        return None;
    }

    let left = (x.floor() - AA_GUARD_PIXELS).clamp(0.0, viewport_width);
    let top = (y.floor() - AA_GUARD_PIXELS).clamp(0.0, viewport_height);
    let right = (right.ceil() + AA_GUARD_PIXELS).clamp(0.0, viewport_width);
    let bottom = (bottom.ceil() + AA_GUARD_PIXELS).clamp(0.0, viewport_height);
    if right <= left || bottom <= top {
        return None;
    }

    let left = left as i32;
    let top = top as i32;
    let right = right as i32;
    let bottom = bottom as i32;
    let viewport_height = viewport_height as i32;
    Some([left, viewport_height - bottom, right - left, bottom - top])
}

#[cfg(test)]
pub(crate) fn transformed_corners(
    bounds: Bounds<ScaledPixels>,
    transform: TransformationMatrix,
) -> [[f32; 2]; 4] {
    let x = bounds.origin.x.0;
    let y = bounds.origin.y.0;
    let width = bounds.size.width.0;
    let height = bounds.size.height.0;
    let points = [
        [x, y],
        [x + width, y],
        [x, y + height],
        [x + width, y + height],
    ];
    points.map(|[x, y]| {
        [
            transform.rotation_scale[0][0] * x
                + transform.rotation_scale[0][1] * y
                + transform.translation[0],
            transform.rotation_scale[1][0] * x
                + transform.rotation_scale[1][1] * y
                + transform.translation[1],
        ]
    })
}

/// Allocation-free iterator over bounded draw ranges.
#[derive(Clone, Debug)]
pub(crate) struct DrawRanges {
    len: usize,
    max_per_draw: usize,
    next: usize,
}

impl Iterator for DrawRanges {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.len {
            return None;
        }
        let start = self.next;
        let end = start.saturating_add(self.max_per_draw).min(self.len);
        self.next = end;
        Some(start..end)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.next);
        let count = remaining.div_ceil(self.max_per_draw);
        (count, Some(count))
    }
}

impl ExactSizeIterator for DrawRanges {}

/// Split a CPU primitive batch into bounded draw ranges without allocating.
/// The browser renderer uses this for upload-sized path batches and can reuse
/// it for instanced primitive uploads without changing draw ordering.
pub(crate) fn draw_ranges(len: usize, max_per_draw: usize) -> DrawRanges {
    assert!(max_per_draw > 0);
    DrawRanges {
        len,
        max_per_draw,
        next: 0,
    }
}

/// Detect well-known software WebGL implementations from privacy-safe base strings.
///
/// Browsers may redact these strings. An unknown or redacted renderer is therefore
/// reported as hardware-backed rather than requiring a fingerprinting extension.
pub(crate) fn is_software_renderer(device: &str, vendor: &str, version: &str) -> bool {
    const SOFTWARE_MARKERS: [&str; 6] = [
        "swiftshader",
        "llvmpipe",
        "softpipe",
        "software rasterizer",
        "software renderer",
        "microsoft basic render",
    ];
    let device = device.to_ascii_lowercase();
    let vendor = vendor.to_ascii_lowercase();
    let version = version.to_ascii_lowercase();
    SOFTWARE_MARKERS.iter().any(|marker| {
        device.contains(marker) || vendor.contains(marker) || version.contains(marker)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hsla, point, size};

    #[test]
    fn batches_cover_every_primitive_without_overlap() {
        assert_eq!(
            draw_ranges(0, 64).collect::<Vec<_>>(),
            Vec::<Range<usize>>::new()
        );
        assert_eq!(
            draw_ranges(5, 2).collect::<Vec<_>>(),
            vec![0..2, 2..4, 4..5]
        );
        let ranges = draw_ranges(1_000_000, 4_095);
        assert_eq!(ranges.len(), 245);
    }

    #[test]
    fn software_renderer_detection_uses_base_webgl_strings() {
        assert!(is_software_renderer(
            "ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device))",
            "Google Inc.",
            "WebGL 2.0"
        ));
        assert!(is_software_renderer(
            "llvmpipe (LLVM 18.1.8, 256 bits)",
            "Mesa",
            "WebGL 2.0"
        ));
        assert!(!is_software_renderer("WebKit WebGL", "WebKit", "WebGL 2.0"));
    }

    #[test]
    fn geometry_conversion_applies_affine_transform() {
        let bounds = Bounds::new(
            point(ScaledPixels(1.0), ScaledPixels(2.0)),
            size(ScaledPixels(3.0), ScaledPixels(4.0)),
        );
        let transform = TransformationMatrix {
            rotation_scale: [[2.0, 0.0], [0.0, 3.0]],
            translation: [5.0, 7.0],
        };
        assert_eq!(
            transformed_corners(bounds, transform),
            [[7.0, 13.0], [13.0, 13.0], [7.0, 25.0], [13.0, 25.0]]
        );
    }

    #[test]
    fn hsla_color_conversion_preserves_alpha_and_primary_color() {
        let rgba = rgba_components(Hsla {
            h: 0.0,
            s: 1.0,
            l: 0.5,
            a: 0.25,
        });
        assert_eq!(rgba, [1.0, 0.0, 0.0, 0.25]);
    }

    #[test]
    fn damage_scissor_expands_clips_and_flips_y() {
        assert_eq!(
            damage_scissor([10.25, 20.75, 30.5, 40.5], [100.0, 100.0]),
            Some([8, 36, 35, 46])
        );
        assert_eq!(
            damage_scissor([-10.0, -5.0, 20.0, 15.0], [100.0, 100.0]),
            Some([0, 88, 12, 12])
        );
    }

    #[test]
    fn damage_scissor_fails_closed_for_invalid_or_empty_input() {
        assert_eq!(
            damage_scissor([f32::NAN, 0.0, 1.0, 1.0], [100.0, 100.0]),
            None
        );
        assert_eq!(
            damage_scissor([200.0, 200.0, 10.0, 10.0], [100.0, 100.0]),
            None
        );
        assert_eq!(damage_scissor([0.0, 0.0, -1.0, 1.0], [100.0, 100.0]), None);
    }

    #[test]
    fn pixel_sample_rejects_uniform_or_translucent_frames() {
        let uniform = [12, 18, 24, 255].repeat(100);
        let stats = analyze_rgba_sample(&uniform).unwrap();
        assert_eq!(stats.changed_pixels, 0);
        assert!(!stats.is_visually_varied(100));

        let translucent = [12, 18, 24, 128].repeat(100);
        let stats = analyze_rgba_sample(&translucent).unwrap();
        assert_eq!(stats.nontransparent_pixels, 0);
        assert!(!stats.is_visually_varied(100));
    }

    #[test]
    fn pixel_sample_accepts_a_varied_opaque_frame() {
        let mut pixels = [12, 18, 24, 255].repeat(10_000);
        for pixel in pixels.chunks_exact_mut(4).skip(1).take(128) {
            pixel.copy_from_slice(&[220, 230, 240, 255]);
        }
        let stats = analyze_rgba_sample(&pixels).unwrap();
        assert_eq!(stats.changed_pixels, 128);
        assert_eq!(stats.nontransparent_pixels, 10_000);
        assert!(stats.luma_range() >= 8);
        assert!(stats.is_visually_varied(10_000));
    }

    #[test]
    fn framebuffer_difference_is_exact_and_rejects_size_changes() {
        assert_eq!(differing_sample_bytes(&[1, 2, 3, 4], &[1, 2, 3, 4]), 0);
        assert_eq!(differing_sample_bytes(&[1, 2, 3, 4], &[1, 9, 3, 8]), 2);
        assert_eq!(differing_sample_bytes(&[1, 2], &[1, 2, 3]), 3);
    }

    #[test]
    fn refresh_rate_estimator_learns_high_refresh_displays() {
        let mut estimator = RefreshRateEstimator::default();
        for frame in 0..16 {
            estimator.record(frame as f64 * (1_000.0 / 120.0));
        }
        let hz = estimator.refresh_rate_hz().unwrap();
        assert!((hz - 120.0).abs() < 0.1, "estimated {hz} Hz");
    }

    #[test]
    fn refresh_rate_estimator_ignores_idle_gaps_and_dropped_frames() {
        let mut estimator = RefreshRateEstimator::default();
        let mut timestamp = 0.0;
        estimator.record(timestamp);
        for frame in 0..12 {
            timestamp += if frame == 5 { 1_000.0 } else { 1_000.0 / 60.0 };
            estimator.record(timestamp);
        }
        timestamp += 1_000.0 / 30.0;
        estimator.record(timestamp);
        let hz = estimator.refresh_rate_hz().unwrap();
        assert!((hz - 60.0).abs() < 0.1, "estimated {hz} Hz");
    }

    #[test]
    fn refresh_rate_estimator_does_not_invent_a_default() {
        let mut estimator = RefreshRateEstimator::default();
        estimator.record(0.0);
        estimator.record(16.0);
        estimator.record(f64::NAN);
        assert_eq!(estimator.refresh_rate_hz(), None);
    }
}
