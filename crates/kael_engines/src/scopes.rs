//! Video scopes — per-channel and luma histograms over a rendered frame.
//!
//! Histograms back the exposure/scopes UI and drive analysis like auto-levels (via
//! [`Histogram::percentile`]). Channels are quantized to 256 bins; luma uses the
//! Rec.709 weighting.

use kael_render_graph::reference::Image;

/// 256-bin histograms of an image's red, green, blue, and luma channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    /// Red-channel bin counts.
    pub red: [u32; 256],
    /// Green-channel bin counts.
    pub green: [u32; 256],
    /// Blue-channel bin counts.
    pub blue: [u32; 256],
    /// Rec.709 luma bin counts.
    pub luma: [u32; 256],
}

fn bin(value: f32) -> usize {
    (value.clamp(0.0, 1.0) * 255.0).round() as usize
}

/// Compute the red/green/blue/luma histograms of `image`.
pub fn histogram(image: &Image) -> Histogram {
    let mut histogram = Histogram {
        red: [0; 256],
        green: [0; 256],
        blue: [0; 256],
        luma: [0; 256],
    };
    for pixel in &image.pixels {
        histogram.red[bin(pixel[0])] += 1;
        histogram.green[bin(pixel[1])] += 1;
        histogram.blue[bin(pixel[2])] += 1;
        let luma = 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2];
        histogram.luma[bin(luma)] += 1;
    }
    histogram
}

impl Histogram {
    /// Total samples (equals the pixel count).
    pub fn total(&self) -> u64 {
        self.luma.iter().map(|&count| count as u64).sum()
    }

    /// The bin (`0..=255`) at which the cumulative count of `bins` first reaches
    /// `fraction` of the total — the level used for auto-levels / black-white points.
    /// Returns 0 for an empty image.
    pub fn percentile(bins: &[u32; 256], fraction: f32) -> u8 {
        let total: u64 = bins.iter().map(|&count| count as u64).sum();
        if total == 0 {
            return 0;
        }
        let target = (fraction.clamp(0.0, 1.0) as f64 * total as f64).ceil() as u64;
        let mut cumulative = 0u64;
        for (index, &count) in bins.iter().enumerate() {
            cumulative += count as u64;
            if cumulative >= target {
                return index as u8;
            }
        }
        255
    }
}

/// Compute auto-contrast black/white input levels from the luma histogram: the bins at
/// the `shadow` and `highlight` cumulative fractions (e.g. 0.01 and 0.99 to ignore
/// outliers).
pub fn auto_levels(histogram: &Histogram, shadow: f32, highlight: f32) -> (u8, u8) {
    (
        Histogram::percentile(&histogram.luma, shadow),
        Histogram::percentile(&histogram.luma, highlight),
    )
}

/// Apply a levels stretch to `image`: map input `black/255` to 0 and `white/255` to 1,
/// clamping outside the range. Alpha is preserved. A zero-width range leaves the image
/// unchanged.
pub fn apply_auto_levels(image: &Image, black: u8, white: u8) -> Image {
    let black_norm = black as f32 / 255.0;
    let white_norm = white as f32 / 255.0;
    let range = white_norm - black_norm;
    let mut output = Image::new(image.width, image.height);
    if range <= f32::EPSILON {
        output.pixels = image.pixels.clone();
        return output;
    }
    output.pixels = image
        .pixels
        .iter()
        .map(|pixel| {
            let remap = |value: f32| ((value - black_norm) / range).clamp(0.0, 1.0);
            [remap(pixel[0]), remap(pixel[1]), remap(pixel[2]), pixel[3]]
        })
        .collect();
    output
}

/// A waveform monitor: for each image column, the count of pixels at each of 256 luma
/// levels — the broadcast scope showing how brightness is distributed horizontally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    /// Number of columns (image width).
    pub width: u32,
    /// Counts indexed `column * 256 + level`.
    pub levels: Vec<u32>,
}

impl Waveform {
    /// The count of pixels in `column` at luma `level`.
    pub fn at(&self, column: u32, level: u8) -> u32 {
        self.levels[column as usize * 256 + level as usize]
    }
}

/// Compute the [`Waveform`] of `image` (Rec.709 luma per column).
pub fn waveform(image: &Image) -> Waveform {
    let mut levels = vec![0u32; image.width as usize * 256];
    for y in 0..image.height {
        for x in 0..image.width {
            let pixel = image.pixel(x, y);
            let luma = 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2];
            let level = (luma.clamp(0.0, 1.0) * 255.0).round() as usize;
            levels[x as usize * 256 + level] += 1;
        }
    }
    Waveform {
        width: image.width,
        levels,
    }
}

/// A vectorscope: a 2-D chroma histogram. Neutral colors land at the center; saturated
/// colors spread outward by hue. Cells are indexed `cr_cell * size + cb_cell`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vectorscope {
    /// Cells per axis.
    pub size: u32,
    /// `size * size` chroma cell counts.
    pub cells: Vec<u32>,
}

impl Vectorscope {
    /// The count in the cell at `(cb_cell, cr_cell)`.
    pub fn at(&self, cb_cell: u32, cr_cell: u32) -> u32 {
        self.cells[(cr_cell * self.size + cb_cell) as usize]
    }
}

/// Compute the [`Vectorscope`] of `image` on a `size`×`size` chroma grid using BT.709
/// Cb/Cr. `size` is clamped to at least 2.
pub fn vectorscope(image: &Image, size: u32) -> Vectorscope {
    let size = size.max(2);
    let axis = (size - 1) as f32;
    let mut cells = vec![0u32; (size * size) as usize];
    const KR: f32 = 0.2126;
    const KB: f32 = 0.0722;
    for pixel in &image.pixels {
        let (r, g, b) = (pixel[0], pixel[1], pixel[2]);
        let luma = KR * r + (1.0 - KR - KB) * g + KB * b;
        // Cb/Cr normalized to 0..=1 with neutral at 0.5.
        let cb = (0.5 * (b - luma) / (1.0 - KB) + 0.5).clamp(0.0, 1.0);
        let cr = (0.5 * (r - luma) / (1.0 - KR) + 0.5).clamp(0.0, 1.0);
        let cb_cell = (cb * axis).round() as u32;
        let cr_cell = (cr * axis).round() as u32;
        cells[(cr_cell * size + cb_cell) as usize] += 1;
    }
    Vectorscope { size, cells }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_color_fills_one_bin_per_channel() {
        let image = Image::filled(4, 4, [1.0, 0.0, 0.0, 1.0]);
        let histogram = histogram(&image);
        assert_eq!(histogram.total(), 16);
        assert_eq!(histogram.red[255], 16);
        assert_eq!(histogram.green[0], 16);
        assert_eq!(histogram.blue[0], 16);
        // Pure red luma is 0.2126 -> bin round(0.2126*255)=54.
        assert_eq!(histogram.luma[54], 16);
    }

    #[test]
    fn black_and_white_land_at_the_extremes() {
        let black = histogram(&Image::filled(2, 2, [0.0, 0.0, 0.0, 1.0]));
        assert_eq!(black.luma[0], 4);
        let white = histogram(&Image::filled(2, 2, [1.0, 1.0, 1.0, 1.0]));
        assert_eq!(white.luma[255], 4);
    }

    #[test]
    fn percentile_finds_levels() {
        // Half the pixels black, half white.
        let mut image = Image::new(4, 1);
        image.pixels = vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ];
        let histogram = histogram(&image);
        // 50% cumulative is reached within the black bin.
        assert_eq!(Histogram::percentile(&histogram.luma, 0.5), 0);
        // Anything beyond half requires the white bin.
        assert_eq!(Histogram::percentile(&histogram.luma, 0.75), 255);
        // The very top is the white point.
        assert_eq!(Histogram::percentile(&histogram.luma, 1.0), 255);
    }

    #[test]
    fn empty_image_percentile_is_zero() {
        let histogram = histogram(&Image::new(0, 0));
        assert_eq!(histogram.total(), 0);
        assert_eq!(Histogram::percentile(&histogram.luma, 0.5), 0);
    }

    #[test]
    fn auto_levels_stretches_a_low_contrast_image() {
        // Half the pixels at gray 0.25 (luma bin 64), half at 0.75 (bin 191).
        let mut image = Image::new(2, 1);
        image.pixels = vec![[0.25, 0.25, 0.25, 1.0], [0.75, 0.75, 0.75, 1.0]];
        let (black, white) = auto_levels(&histogram(&image), 0.25, 0.75);
        assert_eq!((black, white), (64, 191));

        // Applying the stretch maps the dark gray to ~0 and the light gray to ~1.
        let stretched = apply_auto_levels(&image, black, white);
        assert!(
            stretched.pixel(0, 0)[0] < 0.01,
            "{:?}",
            stretched.pixel(0, 0)
        );
        assert!(
            stretched.pixel(1, 0)[0] > 0.99,
            "{:?}",
            stretched.pixel(1, 0)
        );
        // Alpha is preserved.
        assert_eq!(stretched.pixel(0, 0)[3], 1.0);
    }

    #[test]
    fn apply_auto_levels_zero_range_is_passthrough() {
        let image = Image::filled(2, 2, [0.3, 0.4, 0.5, 1.0]);
        let out = apply_auto_levels(&image, 128, 128);
        assert_eq!(out.pixels, image.pixels);
    }

    #[test]
    fn waveform_bins_each_column_by_luma() {
        // Three columns over two rows: black, mid-gray, white.
        let mut image = Image::new(3, 2);
        image.pixels = vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ];
        let scope = waveform(&image);
        assert_eq!(scope.width, 3);
        // Each column has both rows at a single level.
        assert_eq!(scope.at(0, 0), 2);
        assert_eq!(scope.at(1, 128), 2); // round(0.5*255) = 128
        assert_eq!(scope.at(2, 255), 2);
        // Nothing elsewhere in those columns.
        assert_eq!(scope.at(0, 255), 0);
    }

    #[test]
    fn vectorscope_centers_neutrals_and_spreads_saturated_colors() {
        // A neutral gray lands at the center cell (size 16 -> 0.5*15 rounds to 8).
        let gray = vectorscope(&Image::filled(4, 4, [0.5, 0.5, 0.5, 1.0]), 16);
        assert_eq!(gray.at(8, 8), 16);
        assert_eq!(gray.cells.iter().sum::<u32>(), 16);

        // Saturated red leaves the center and sits high on the Cr (red) axis.
        let red = vectorscope(&Image::filled(2, 2, [1.0, 0.0, 0.0, 1.0]), 16);
        assert_eq!(red.at(8, 8), 0);
        assert_eq!(red.cells.iter().sum::<u32>(), 4);
        let populated = red.cells.iter().position(|&count| count > 0).unwrap();
        assert!(
            populated / 16 > 8,
            "red should be high Cr: cr_cell={}",
            populated / 16
        );
    }
}
