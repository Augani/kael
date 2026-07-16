//! Video scopes — per-channel and luma histograms over a rendered frame.
//!
//! Histograms back the exposure/scopes UI and drive analysis like auto-levels (via
//! [`Histogram::percentile`]). Channels are quantized to 256 bins; luma uses the
//! Rec.709 weighting.

use kael_render_graph::reference::Image;

const MAX_SCOPE_CELLS: usize = 4 * 1024 * 1024;

fn zeroed_scope_cells(length: usize) -> Vec<u32> {
    if length > MAX_SCOPE_CELLS {
        return Vec::new();
    }
    let mut cells = Vec::new();
    if cells.try_reserve_exact(length).is_err() {
        return Vec::new();
    }
    cells.resize(length, 0);
    cells
}

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
        histogram.red[bin(pixel[0])] = histogram.red[bin(pixel[0])].saturating_add(1);
        histogram.green[bin(pixel[1])] = histogram.green[bin(pixel[1])].saturating_add(1);
        histogram.blue[bin(pixel[2])] = histogram.blue[bin(pixel[2])].saturating_add(1);
        let luma = 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2];
        histogram.luma[bin(luma)] = histogram.luma[bin(luma)].saturating_add(1);
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
        usize::try_from(column)
            .ok()
            .and_then(|column| column.checked_mul(256))
            .and_then(|base| base.checked_add(usize::from(level)))
            .and_then(|index| self.levels.get(index))
            .copied()
            .unwrap_or(0)
    }
}

/// Compute the [`Waveform`] of `image` (Rec.709 luma per column).
pub fn waveform(image: &Image) -> Waveform {
    let length = usize::try_from(image.width)
        .ok()
        .and_then(|width| width.checked_mul(256))
        .unwrap_or(usize::MAX);
    let mut levels = zeroed_scope_cells(length);
    if levels.is_empty() && image.width != 0 {
        return Waveform {
            width: image.width,
            levels,
        };
    }
    for y in 0..image.height {
        for x in 0..image.width {
            let pixel = image.pixel(x, y);
            let luma = 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2];
            let level = (luma.clamp(0.0, 1.0) * 255.0).round() as usize;
            let count = &mut levels[x as usize * 256 + level];
            *count = count.saturating_add(1);
        }
    }
    Waveform {
        width: image.width,
        levels,
    }
}

/// An RGB parade: three per-channel column distributions (red, green, blue) shown side by
/// side — the grading scope for checking white balance and per-channel clipping, unlike the
/// single-channel luma [`Waveform`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parade {
    /// Number of columns (image width).
    pub width: u32,
    /// Red-channel counts indexed `column * 256 + level`.
    pub red: Vec<u32>,
    /// Green-channel counts indexed `column * 256 + level`.
    pub green: Vec<u32>,
    /// Blue-channel counts indexed `column * 256 + level`.
    pub blue: Vec<u32>,
}

impl Parade {
    /// The count of pixels in `column` whose `channel` (0 = red, 1 = green, else blue) sits
    /// at `level`.
    pub fn at(&self, channel: usize, column: u32, level: u8) -> u32 {
        let plane = match channel {
            0 => &self.red,
            1 => &self.green,
            _ => &self.blue,
        };
        usize::try_from(column)
            .ok()
            .and_then(|column| column.checked_mul(256))
            .and_then(|base| base.checked_add(usize::from(level)))
            .and_then(|index| plane.get(index))
            .copied()
            .unwrap_or(0)
    }
}

/// Compute the [`Parade`] of `image` — a per-RGB-channel horizontal distribution.
pub fn parade(image: &Image) -> Parade {
    let width = usize::try_from(image.width).unwrap_or(usize::MAX);
    let length = width.saturating_mul(256);
    let mut red = zeroed_scope_cells(length);
    let mut green = zeroed_scope_cells(length);
    let mut blue = zeroed_scope_cells(length);
    if (red.is_empty() || green.is_empty() || blue.is_empty()) && image.width != 0 {
        return Parade {
            width: image.width,
            red,
            green,
            blue,
        };
    }
    let level = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as usize;
    for y in 0..image.height {
        for x in 0..image.width {
            let pixel = image.pixel(x, y);
            let column = x as usize * 256;
            let red_count = &mut red[column + level(pixel[0])];
            *red_count = red_count.saturating_add(1);
            let green_count = &mut green[column + level(pixel[1])];
            *green_count = green_count.saturating_add(1);
            let blue_count = &mut blue[column + level(pixel[2])];
            *blue_count = blue_count.saturating_add(1);
        }
    }
    Parade {
        width: image.width,
        red,
        green,
        blue,
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
        if cb_cell >= self.size || cr_cell >= self.size {
            return 0;
        }
        cr_cell
            .checked_mul(self.size)
            .and_then(|row| row.checked_add(cb_cell))
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.cells.get(index))
            .copied()
            .unwrap_or(0)
    }
}

/// Compute the [`Vectorscope`] of `image` on a `size`×`size` chroma grid using BT.709
/// Cb/Cr. `size` is clamped to `2..=2048` to bound analysis memory.
pub fn vectorscope(image: &Image, size: u32) -> Vectorscope {
    let size = size.clamp(2, 2048);
    let axis = (size - 1) as f32;
    let cell_count = usize::try_from(size)
        .ok()
        .and_then(|size| size.checked_mul(size))
        .unwrap_or(usize::MAX);
    let mut cells = zeroed_scope_cells(cell_count);
    if cells.is_empty() {
        return Vectorscope { size, cells };
    }
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
        let count = &mut cells[(cr_cell * size + cb_cell) as usize];
        *count = count.saturating_add(1);
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
    fn parade_separates_rgb_channels_per_column() {
        // One pixel with a distinct value per channel.
        let mut image = Image::new(1, 1);
        image.pixels = vec![[1.0, 0.5, 0.0, 1.0]];
        let scope = parade(&image);
        assert_eq!(scope.width, 1);
        // Red at 255, green at round(0.5*255)=128, blue at 0.
        assert_eq!(scope.at(0, 0, 255), 1);
        assert_eq!(scope.at(1, 0, 128), 1);
        assert_eq!(scope.at(2, 0, 0), 1);
        // Channels don't bleed into each other's levels.
        assert_eq!(scope.at(0, 0, 0), 0);
        assert_eq!(scope.at(2, 0, 255), 0);
    }

    #[test]
    fn parade_counts_every_pixel_in_a_column() {
        // Two rows, one column; each channel accumulates both rows at its level.
        let image = Image::filled(1, 2, [0.25, 0.25, 0.25, 1.0]);
        let scope = parade(&image);
        let level = (0.25_f32 * 255.0).round() as u8; // 64
        assert_eq!(scope.at(0, 0, level), 2);
        assert_eq!(scope.at(1, 0, level), 2);
        assert_eq!(scope.at(2, 0, level), 2);
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

    #[test]
    fn scope_accessors_return_zero_outside_the_grid() {
        let image = Image::filled(1, 1, [0.5, 0.5, 0.5, 1.0]);
        let waveform = waveform(&image);
        let parade = parade(&image);
        let vectorscope = vectorscope(&image, 16);

        assert_eq!(waveform.at(u32::MAX, 0), 0);
        assert_eq!(parade.at(0, u32::MAX, 0), 0);
        assert_eq!(vectorscope.at(16, 0), 0);
        assert_eq!(vectorscope.at(0, 16), 0);
    }

    #[test]
    fn vectorscope_bounds_requested_grid_size() {
        let scope = vectorscope(&Image::new(0, 0), u32::MAX);
        assert_eq!(scope.size, 2048);
        assert_eq!(scope.cells.len(), 2048 * 2048);
    }
}
