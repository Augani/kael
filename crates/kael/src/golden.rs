//! Golden-image pixel-diff comparison for the headless render pipeline (P0-J).
//!
//! GPU rasterization is not bit-exact across drivers and backends, so a golden test
//! cannot demand byte equality. This module compares a rendered frame to a reference
//! under an explicit [`Tolerance`](crate::golden::Tolerance): a per-channel difference
//! threshold and a cap on the fraction of pixels allowed to exceed it. The comparison
//! is a pure function over 8-bit, 4-channel byte buffers (the format the off-screen
//! readback produces), so it runs identically on every platform.

use anyhow::{Result, bail};

const MAX_GOLDEN_BUFFER_BYTES: usize = 256 * 1024 * 1024;

/// How much a rendered frame may differ from its golden reference and still pass.
///
/// A pixel "fails" when its largest absolute per-channel difference exceeds
/// [`Tolerance::per_channel`]; the frame passes when the fraction of failing pixels
/// does not exceed [`Tolerance::max_failing_fraction`]. The two knobs separate
/// *how far off* a pixel may be from *how many* pixels may be off — anti-aliased
/// edges produce a few large-diff pixels even on a correct render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Maximum allowed absolute per-channel difference before a pixel is "failing".
    pub per_channel: u8,
    /// Maximum fraction (`0.0..=1.0`) of failing pixels the frame may contain.
    pub max_failing_fraction: f64,
}

impl Tolerance {
    /// Creates a tolerance after validating its failing-pixel fraction.
    pub fn new_checked(per_channel: u8, max_failing_fraction: f64) -> Result<Self> {
        let tolerance = Self {
            per_channel,
            max_failing_fraction,
        };
        tolerance.validate()?;
        Ok(tolerance)
    }

    /// Demand an exact, byte-identical match (used for CPU reference oracles).
    pub const fn exact() -> Self {
        Self {
            per_channel: 0,
            max_failing_fraction: 0.0,
        }
    }

    /// A lenient policy suited to GPU rasterization differences: small per-channel
    /// drift on up to 1% of pixels (typically anti-aliased edges).
    pub const fn gpu() -> Self {
        Self {
            per_channel: 2,
            max_failing_fraction: 0.01,
        }
    }

    fn validate(&self) -> Result<()> {
        if !self.max_failing_fraction.is_finite()
            || !(0.0..=1.0).contains(&self.max_failing_fraction)
        {
            bail!("golden tolerance fraction must be finite and between zero and one");
        }
        Ok(())
    }
}

/// The outcome of comparing a rendered frame to a golden reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffReport {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Total pixels compared (`width * height`).
    pub total_pixels: u64,
    /// Pixels whose largest per-channel difference exceeded the tolerance threshold.
    pub failing_pixels: u64,
    /// Largest single per-channel difference seen anywhere in the frame.
    pub max_channel_diff: u8,
    /// Mean per-channel difference across every channel of every pixel.
    pub mean_channel_diff: f64,
}

impl DiffReport {
    /// The fraction of pixels that exceeded the per-channel threshold.
    pub fn failing_fraction(&self) -> f64 {
        if self.total_pixels == 0 {
            return 0.0;
        }
        self.failing_pixels as f64 / self.total_pixels as f64
    }

    /// Whether the rendered frame is within tolerance of the golden reference.
    pub fn passes(&self, tolerance: &Tolerance) -> bool {
        tolerance.validate().is_ok()
            && self.failing_pixels <= self.total_pixels
            && self.failing_fraction() <= tolerance.max_failing_fraction
    }
}

/// Compare a rendered frame to a golden reference under `tolerance`.
///
/// Both buffers must be 8-bit, 4-channel, tightly packed at `width * height * 4`
/// bytes; the channel order is irrelevant as long as both buffers share it. A pixel
/// is counted as failing when the maximum absolute difference across its four
/// channels exceeds [`Tolerance::per_channel`].
///
/// Returns an error if the dimensions are zero or either buffer length is wrong.
pub fn compare(
    actual: &[u8],
    expected: &[u8],
    width: u32,
    height: u32,
    tolerance: &Tolerance,
) -> Result<DiffReport> {
    tolerance.validate()?;
    let expected_len = checked_rgba_len(width, height)?;
    if actual.len() != expected_len {
        bail!(
            "actual buffer is {} bytes, expected {} ({}x{}x4)",
            actual.len(),
            expected_len,
            width,
            height
        );
    }
    if expected.len() != expected_len {
        bail!(
            "reference buffer is {} bytes, expected {} ({}x{}x4)",
            expected.len(),
            expected_len,
            width,
            height
        );
    }

    let total_pixels = width as u64 * height as u64;
    let mut failing_pixels = 0u64;
    let mut max_channel_diff = 0u8;
    let mut diff_sum = 0u64;

    for (actual_pixel, expected_pixel) in actual.chunks_exact(4).zip(expected.chunks_exact(4)) {
        let mut pixel_max = 0u8;
        for channel in 0..4 {
            let diff = actual_pixel[channel].abs_diff(expected_pixel[channel]);
            diff_sum += diff as u64;
            if diff > pixel_max {
                pixel_max = diff;
            }
        }
        if pixel_max > max_channel_diff {
            max_channel_diff = pixel_max;
        }
        if pixel_max > tolerance.per_channel {
            failing_pixels += 1;
        }
    }

    let mean_channel_diff = diff_sum as f64 / (total_pixels as f64 * 4.0);

    Ok(DiffReport {
        width,
        height,
        total_pixels,
        failing_pixels,
        max_channel_diff,
        mean_channel_diff,
    })
}

/// Build a solid-color reference buffer for tests that render a known fill.
///
/// `channels` is written verbatim for every pixel, so the caller chooses the byte
/// order (e.g. `[b, g, r, a]` to match a BGRA readback).
pub fn solid_reference(width: u32, height: u32, channels: [u8; 4]) -> Vec<u8> {
    solid_reference_checked(width, height, channels).unwrap_or_default()
}

/// Builds a bounded solid-color reference or reports invalid dimensions.
pub fn solid_reference_checked(width: u32, height: u32, channels: [u8; 4]) -> Result<Vec<u8>> {
    let byte_len = checked_rgba_len(width, height)?;
    let mut buffer = Vec::with_capacity(byte_len);
    for _ in 0..(width as u64 * height as u64) {
        buffer.extend_from_slice(&channels);
    }
    Ok(buffer)
}

fn checked_rgba_len(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        bail!("golden image requires non-zero dimensions");
    }
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| anyhow::anyhow!("golden image dimensions overflow addressable memory"))?;
    if byte_len > MAX_GOLDEN_BUFFER_BYTES {
        bail!("golden image cannot exceed {MAX_GOLDEN_BUFFER_BYTES} bytes");
    }
    Ok(byte_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_buffers_have_zero_diff_and_pass_exact() {
        let frame = solid_reference(4, 4, [10, 20, 30, 255]);
        let report = compare(&frame, &frame, 4, 4, &Tolerance::exact()).unwrap();
        assert_eq!(report.failing_pixels, 0);
        assert_eq!(report.max_channel_diff, 0);
        assert_eq!(report.mean_channel_diff, 0.0);
        assert!(report.passes(&Tolerance::exact()));
    }

    #[test]
    fn single_off_pixel_is_counted_and_gated_by_fraction() {
        let reference = solid_reference(2, 2, [0, 0, 0, 255]);
        let mut actual = reference.clone();
        // Nudge the first pixel's green channel by 5.
        actual[1] = 5;

        let report = compare(&actual, &reference, 2, 2, &Tolerance::exact()).unwrap();
        assert_eq!(report.failing_pixels, 1);
        assert_eq!(report.total_pixels, 4);
        assert_eq!(report.max_channel_diff, 5);
        assert_eq!(report.failing_fraction(), 0.25);

        // Exact tolerance rejects any failing pixel.
        assert!(!report.passes(&Tolerance::exact()));
        // Allowing up to 25% failing pixels passes; allowing the diff per-channel
        // would not even count it as failing.
        assert!(report.passes(&Tolerance {
            per_channel: 0,
            max_failing_fraction: 0.25
        }));
        let lenient = compare(
            &actual,
            &reference,
            2,
            2,
            &Tolerance {
                per_channel: 5,
                max_failing_fraction: 0.0,
            },
        )
        .unwrap();
        assert_eq!(lenient.failing_pixels, 0);
        assert!(lenient.passes(&Tolerance {
            per_channel: 5,
            max_failing_fraction: 0.0
        }));
    }

    #[test]
    fn mean_diff_averages_over_all_channels() {
        let reference = solid_reference(1, 1, [0, 0, 0, 0]);
        let actual = vec![4u8, 0, 0, 0];
        let report = compare(&actual, &reference, 1, 1, &Tolerance::exact()).unwrap();
        // One channel off by 4 across four channels => mean 1.0.
        assert_eq!(report.mean_channel_diff, 1.0);
        assert_eq!(report.max_channel_diff, 4);
    }

    #[test]
    fn gpu_tolerance_absorbs_small_sparse_drift() {
        // 100 pixels, two of them off by 1 — within the 1% / per-channel-2 GPU policy.
        let reference = solid_reference(10, 10, [128, 128, 128, 255]);
        let mut actual = reference.clone();
        actual[0] = 129;
        let report = compare(&actual, &reference, 10, 10, &Tolerance::gpu()).unwrap();
        assert_eq!(
            report.failing_pixels, 0,
            "diff of 1 is within per_channel=2"
        );
        assert!(report.passes(&Tolerance::gpu()));
    }

    #[test]
    fn wrong_buffer_length_is_an_error() {
        let reference = solid_reference(4, 4, [0, 0, 0, 0]);
        let short = vec![0u8; 8];
        assert!(compare(&short, &reference, 4, 4, &Tolerance::exact()).is_err());
        assert!(compare(&reference, &reference, 0, 4, &Tolerance::exact()).is_err());
    }

    #[test]
    fn invalid_tolerances_and_oversized_images_fail_closed() {
        for fraction in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(Tolerance::new_checked(2, fraction).is_err());
            let invalid = Tolerance {
                per_channel: 2,
                max_failing_fraction: fraction,
            };
            assert!(
                !DiffReport {
                    width: 1,
                    height: 1,
                    total_pixels: 1,
                    failing_pixels: 0,
                    max_channel_diff: 0,
                    mean_channel_diff: 0.0,
                }
                .passes(&invalid)
            );
            assert!(compare(&[0; 4], &[0; 4], 1, 1, &invalid).is_err());
        }

        assert!(solid_reference_checked(0, 1, [0; 4]).is_err());
        assert!(solid_reference_checked(u32::MAX, u32::MAX, [0; 4]).is_err());
        assert!(solid_reference(u32::MAX, u32::MAX, [0; 4]).is_empty());
    }
}
