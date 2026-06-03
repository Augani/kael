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

/// A 3x3 matrix in row-major order (`m[row][col]`).
pub type Mat3 = [[f32; 3]; 3];

/// CIE 1931 color primaries identifying an RGB gamut (each with a D65 white point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPrimaries {
    /// BT.709 / sRGB primaries.
    Bt709,
    /// BT.2020 / BT.2100 (UHD) primaries.
    Bt2020,
    /// SMPTE-C (BT.601 525-line) primaries.
    SmpteC,
}

impl ColorPrimaries {
    /// The CIE xy chromaticities `(red, green, blue, white)`.
    pub fn chromaticities(self) -> ([f32; 2], [f32; 2], [f32; 2], [f32; 2]) {
        let d65 = [0.3127, 0.3290];
        match self {
            Self::Bt709 => ([0.640, 0.330], [0.300, 0.600], [0.150, 0.060], d65),
            Self::Bt2020 => ([0.708, 0.292], [0.170, 0.797], [0.131, 0.046], d65),
            Self::SmpteC => ([0.630, 0.340], [0.310, 0.595], [0.155, 0.070], d65),
        }
    }
}

fn chromaticity_to_xyz(chromaticity: [f32; 2]) -> [f32; 3] {
    let (x, y) = (chromaticity[0], chromaticity[1]);
    [x / y, 1.0, (1.0 - x - y) / y]
}

fn mat3_vec(m: Mat3, v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn mat3_mul(a: Mat3, b: Mat3) -> Mat3 {
    let mut out = [[0.0f32; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

fn mat3_inverse(m: Mat3) -> Option<Mat3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ])
}

/// The linear-RGB to CIE-XYZ matrix for the given primaries.
///
/// The middle (Y) row equals the gamut's luma coefficients — e.g. BT.709 yields
/// `(0.2126, 0.7152, 0.0722)`.
pub fn rgb_to_xyz_matrix(primaries: ColorPrimaries) -> Mat3 {
    let (r, g, b, w) = primaries.chromaticities();
    let (xr, xg, xb) = (
        chromaticity_to_xyz(r),
        chromaticity_to_xyz(g),
        chromaticity_to_xyz(b),
    );
    let basis = [
        [xr[0], xg[0], xb[0]],
        [xr[1], xg[1], xb[1]],
        [xr[2], xg[2], xb[2]],
    ];
    let white = chromaticity_to_xyz(w);
    let scale = mat3_vec(
        mat3_inverse(basis).expect("primaries are linearly independent"),
        white,
    );
    [
        [xr[0] * scale[0], xg[0] * scale[1], xb[0] * scale[2]],
        [xr[1] * scale[0], xg[1] * scale[1], xb[1] * scale[2]],
        [xr[2] * scale[0], xg[2] * scale[1], xb[2] * scale[2]],
    ]
}

/// The CIE-XYZ to linear-RGB matrix for the given primaries.
pub fn xyz_to_rgb_matrix(primaries: ColorPrimaries) -> Mat3 {
    mat3_inverse(rgb_to_xyz_matrix(primaries)).expect("rgb-to-xyz matrix is invertible")
}

/// The linear-RGB gamut-conversion matrix from `from` primaries to `to` primaries
/// (e.g. BT.709 → BT.2020), via the shared CIE-XYZ connection space.
pub fn gamut_conversion_matrix(from: ColorPrimaries, to: ColorPrimaries) -> Mat3 {
    mat3_mul(xyz_to_rgb_matrix(to), rgb_to_xyz_matrix(from))
}

#[cfg(test)]
mod primaries_tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn bt709_luma_row_matches_coefficients() {
        let m = rgb_to_xyz_matrix(ColorPrimaries::Bt709);
        assert!(close(m[1][0], 0.2126, 2e-3), "{:?}", m[1]);
        assert!(close(m[1][1], 0.7152, 2e-3), "{:?}", m[1]);
        assert!(close(m[1][2], 0.0722, 2e-3), "{:?}", m[1]);
    }

    #[test]
    fn bt2020_luma_row_matches_coefficients() {
        let m = rgb_to_xyz_matrix(ColorPrimaries::Bt2020);
        assert!(close(m[1][0], 0.2627, 2e-3), "{:?}", m[1]);
        assert!(close(m[1][1], 0.6780, 2e-3), "{:?}", m[1]);
        assert!(close(m[1][2], 0.0593, 2e-3), "{:?}", m[1]);
    }

    #[test]
    fn white_maps_to_d65() {
        let white = mat3_vec(rgb_to_xyz_matrix(ColorPrimaries::Bt709), [1.0, 1.0, 1.0]);
        assert!(close(white[0], 0.9505, 3e-3), "{white:?}");
        assert!(close(white[1], 1.0, 1e-4), "{white:?}");
        assert!(close(white[2], 1.0891, 3e-3), "{white:?}");
    }

    #[test]
    fn gamut_self_conversion_is_identity() {
        let m = gamut_conversion_matrix(ColorPrimaries::Bt709, ColorPrimaries::Bt709);
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(close(m[i][j], expected, 1e-4), "[{i}][{j}] = {}", m[i][j]);
            }
        }
    }

    #[test]
    fn bt709_to_bt2020_preserves_white_and_contains_red() {
        let m = gamut_conversion_matrix(ColorPrimaries::Bt709, ColorPrimaries::Bt2020);
        let white = mat3_vec(m, [1.0, 1.0, 1.0]);
        assert!(
            close(white[0], 1.0, 2e-3) && close(white[1], 1.0, 2e-3) && close(white[2], 1.0, 2e-3)
        );
        let red = mat3_vec(m, [1.0, 0.0, 0.0]);
        assert!(red[0] > 0.6 && red[0] < 1.0, "709 red in 2020: {red:?}");
        assert!(
            red[1].abs() < 0.1 && red[2].abs() < 0.1,
            "709 red in 2020: {red:?}"
        );
    }
}
