//! Video color: YCbCr→RGB matrices and transfer functions.
//!
//! The macOS surface shader currently hardcodes a single BT.601 full-range
//! YCbCr matrix, which produces wrong colors for HD (BT.709) and UHD (BT.2020)
//! footage. This module computes the correct 4×4 conversion matrix for any
//! combination of [`VideoMatrixCoefficients`], [`VideoColorRange`], and bit
//! depth, in the column-major layout the surface shaders consume, so the
//! display path can dispatch on a clip's signalled colorimetry instead of
//! assuming one format. It also provides the standard transfer functions used
//! to move between gamma-encoded and linear light.

/// Matrix coefficients identifying the YCbCr↔RGB relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoMatrixCoefficients {
    /// BT.601 (SD): Kr = 0.299, Kb = 0.114.
    Bt601,
    /// BT.709 (HD): Kr = 0.2126, Kb = 0.0722.
    Bt709,
    /// BT.2020 non-constant luminance (UHD): Kr = 0.2627, Kb = 0.0593.
    Bt2020Ncl,
}

impl VideoMatrixCoefficients {
    /// The luma coefficients `(Kr, Kg, Kb)` with `Kg = 1 − Kr − Kb`.
    pub fn luma_coefficients(self) -> (f32, f32, f32) {
        let (kr, kb) = match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
            Self::Bt2020Ncl => (0.2627, 0.0593),
        };
        (kr, 1.0 - kr - kb, kb)
    }
}

/// Signal range of the YCbCr samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoColorRange {
    /// "TV"/studio range: luma in `[16, 235]·2^(bd−8)`, chroma in `[16, 240]·2^(bd−8)`.
    Limited,
    /// "PC"/full range: the entire `[0, 2^bd − 1]` code range.
    Full,
}

struct RangeNormalization {
    luma_offset: f32,
    luma_scale: f32,
    chroma_neutral: f32,
    chroma_scale: f32,
}

fn range_normalization(range: VideoColorRange, bit_depth: u8) -> RangeNormalization {
    match range {
        VideoColorRange::Full => RangeNormalization {
            luma_offset: 0.0,
            luma_scale: 1.0,
            chroma_neutral: 0.5,
            chroma_scale: 1.0,
        },
        VideoColorRange::Limited => {
            let bit_depth = bit_depth.clamp(8, 16) as i32;
            let max = ((1u32 << bit_depth) - 1) as f32;
            let step = (1u32 << (bit_depth - 8)) as f32;
            RangeNormalization {
                luma_offset: 16.0 * step / max,
                luma_scale: max / (219.0 * step),
                chroma_neutral: 128.0 * step / max,
                chroma_scale: max / (224.0 * step),
            }
        }
    }
}

/// Build the column-major 4×4 matrix that maps normalized YCbCr samples
/// `(Y, Cb, Cr, 1)` (each in `0..=1`, as sampled from the video textures) to
/// gamma-encoded R'G'B' in `0..=1`.
///
/// The layout matches the surface shaders: `rgb = matrix * vec4(y, cb, cr, 1)`,
/// with `matrix[col][row]`.
pub fn ycbcr_to_rgb_matrix(
    coefficients: VideoMatrixCoefficients,
    range: VideoColorRange,
    bit_depth: u8,
) -> [[f32; 4]; 4] {
    let (kr, kg, kb) = coefficients.luma_coefficients();
    let norm = range_normalization(range, bit_depth);

    let r_from_cr = 2.0 * (1.0 - kr);
    let b_from_cb = 2.0 * (1.0 - kb);
    let g_from_cb = -2.0 * kb * (1.0 - kb) / kg;
    let g_from_cr = -2.0 * kr * (1.0 - kr) / kg;

    let ys = norm.luma_scale;
    let cs = norm.chroma_scale;

    let r_off = -ys * norm.luma_offset - r_from_cr * cs * norm.chroma_neutral;
    let g_off = -ys * norm.luma_offset - (g_from_cb + g_from_cr) * cs * norm.chroma_neutral;
    let b_off = -ys * norm.luma_offset - b_from_cb * cs * norm.chroma_neutral;

    [
        [ys, ys, ys, 0.0],
        [0.0, g_from_cb * cs, b_from_cb * cs, 0.0],
        [r_from_cr * cs, g_from_cr * cs, 0.0, 0.0],
        [r_off, g_off, b_off, 1.0],
    ]
}

/// Convert normalized YCbCr samples to gamma-encoded R'G'B' using
/// [`ycbcr_to_rgb_matrix`]. Inputs and outputs are in `0..=1`.
pub fn convert_ycbcr(
    coefficients: VideoMatrixCoefficients,
    range: VideoColorRange,
    bit_depth: u8,
    y: f32,
    cb: f32,
    cr: f32,
) -> [f32; 3] {
    let m = ycbcr_to_rgb_matrix(coefficients, range, bit_depth);
    let mut out = [0.0f32; 3];
    let input = [y, cb, cr, 1.0];
    for (row, out_value) in out.iter_mut().enumerate() {
        let mut acc = 0.0;
        for (col, input_value) in input.iter().enumerate() {
            acc += m[col][row] * input_value;
        }
        *out_value = acc;
    }
    out
}

/// Opto-electronic / electro-optical transfer functions for moving between
/// gamma-encoded values and linear light. Inputs/outputs are normalized to `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFunction {
    /// Identity (already linear).
    Linear,
    /// sRGB / IEC 61966-2-1.
    Srgb,
    /// BT.1886 display gamma (≈ 2.4).
    Bt1886,
    /// SMPTE ST 2084 (PQ), normalized so `1.0` maps to 10,000 cd/m².
    Pq,
}

impl TransferFunction {
    /// Decode a gamma-encoded value to linear light.
    pub fn to_linear(self, value: f32) -> f32 {
        let value = value.clamp(0.0, 1.0);
        match self {
            Self::Linear => value,
            Self::Srgb => {
                if value <= 0.040_448_237 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            }
            Self::Bt1886 => value.powf(2.4),
            Self::Pq => {
                const M1: f32 = 0.159_301_76;
                const M2: f32 = 78.84375;
                const C1: f32 = 0.835_937_5;
                const C2: f32 = 18.851_562;
                const C3: f32 = 18.687_5;
                let vp = value.powf(1.0 / M2);
                let num = (vp - C1).max(0.0);
                let den = C2 - C3 * vp;
                (num / den).powf(1.0 / M1)
            }
        }
    }

    /// Encode a linear value with this transfer function.
    pub fn from_linear(self, value: f32) -> f32 {
        let value = value.clamp(0.0, 1.0);
        match self {
            Self::Linear => value,
            Self::Srgb => {
                if value <= 0.003_130_8 {
                    value * 12.92
                } else {
                    1.055 * value.powf(1.0 / 2.4) - 0.055
                }
            }
            Self::Bt1886 => value.powf(1.0 / 2.4),
            Self::Pq => {
                const M1: f32 = 0.159_301_76;
                const M2: f32 = 78.84375;
                const C1: f32 = 0.835_937_5;
                const C2: f32 = 18.851_562;
                const C3: f32 = 18.687_5;
                let vm = value.powf(M1);
                ((C1 + C2 * vm) / (1.0 + C3 * vm)).powf(M2)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn bt601_full_8bit_matches_legacy_shader_matrix() {
        let m = ycbcr_to_rgb_matrix(VideoMatrixCoefficients::Bt601, VideoColorRange::Full, 8);
        // The matrix currently hardcoded in platform/mac/shaders.metal.
        let expected = [
            [1.0, 1.0, 1.0, 0.0],
            [0.0, -0.3441, 1.7720, 0.0],
            [1.4020, -0.7141, 0.0, 0.0],
            [-0.7010, 0.5291, -0.8860, 1.0],
        ];
        for col in 0..4 {
            for row in 0..4 {
                assert!(
                    close(m[col][row], expected[col][row], 2e-3),
                    "mismatch at [{col}][{row}]: {} vs {}",
                    m[col][row],
                    expected[col][row]
                );
            }
        }
    }

    #[test]
    fn limited_range_maps_black_and_white() {
        for coeffs in [
            VideoMatrixCoefficients::Bt601,
            VideoMatrixCoefficients::Bt709,
            VideoMatrixCoefficients::Bt2020Ncl,
        ] {
            let black = convert_ycbcr(
                coeffs,
                VideoColorRange::Limited,
                8,
                16.0 / 255.0,
                128.0 / 255.0,
                128.0 / 255.0,
            );
            let white = convert_ycbcr(
                coeffs,
                VideoColorRange::Limited,
                8,
                235.0 / 255.0,
                128.0 / 255.0,
                128.0 / 255.0,
            );
            for channel in 0..3 {
                assert!(
                    close(black[channel], 0.0, 1e-3),
                    "black {coeffs:?} {black:?}"
                );
                assert!(
                    close(white[channel], 1.0, 1e-3),
                    "white {coeffs:?} {white:?}"
                );
            }
        }
    }

    #[test]
    fn full_range_maps_black_and_white() {
        let black = convert_ycbcr(
            VideoMatrixCoefficients::Bt709,
            VideoColorRange::Full,
            8,
            0.0,
            0.5,
            0.5,
        );
        let white = convert_ycbcr(
            VideoMatrixCoefficients::Bt709,
            VideoColorRange::Full,
            8,
            1.0,
            0.5,
            0.5,
        );
        assert!(black.iter().all(|&c| close(c, 0.0, 1e-4)));
        assert!(white.iter().all(|&c| close(c, 1.0, 1e-4)));
    }

    #[test]
    fn bt709_differs_from_bt601_for_pure_chroma() {
        let red_cr = 0.9;
        let r601 = convert_ycbcr(
            VideoMatrixCoefficients::Bt601,
            VideoColorRange::Full,
            8,
            0.5,
            0.5,
            red_cr,
        );
        let r709 = convert_ycbcr(
            VideoMatrixCoefficients::Bt709,
            VideoColorRange::Full,
            8,
            0.5,
            0.5,
            red_cr,
        );
        assert!(
            (r601[0] - r709[0]).abs() > 1e-2,
            "709 and 601 must differ: {r601:?} vs {r709:?}"
        );
    }

    #[test]
    fn ten_bit_limited_white_point() {
        let white = convert_ycbcr(
            VideoMatrixCoefficients::Bt2020Ncl,
            VideoColorRange::Limited,
            10,
            940.0 / 1023.0,
            512.0 / 1023.0,
            512.0 / 1023.0,
        );
        assert!(white.iter().all(|&c| close(c, 1.0, 2e-3)), "{white:?}");
    }

    #[test]
    fn srgb_transfer_roundtrips_and_known_points() {
        assert!(close(TransferFunction::Srgb.to_linear(0.0), 0.0, 1e-6));
        assert!(close(TransferFunction::Srgb.to_linear(1.0), 1.0, 1e-6));
        assert!(close(
            TransferFunction::Srgb.to_linear(0.5),
            0.214_041,
            1e-3
        ));
        for v in [0.0, 0.05, 0.25, 0.5, 0.75, 1.0] {
            let round = TransferFunction::Srgb.from_linear(TransferFunction::Srgb.to_linear(v));
            assert!(close(round, v, 1e-4), "roundtrip {v} -> {round}");
        }
    }

    #[test]
    fn pq_transfer_endpoints_and_roundtrip() {
        assert!(close(TransferFunction::Pq.to_linear(0.0), 0.0, 1e-4));
        assert!(close(TransferFunction::Pq.to_linear(1.0), 1.0, 1e-3));
        for v in [0.1, 0.4, 0.7, 1.0] {
            let round = TransferFunction::Pq.from_linear(TransferFunction::Pq.to_linear(v));
            assert!(close(round, v, 2e-3), "pq roundtrip {v} -> {round}");
        }
    }
}
