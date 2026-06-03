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
}
