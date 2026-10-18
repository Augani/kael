use std::borrow::Cow;

use color_quant::NeuQuant;
use gif::Frame as GifFrame;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Copy)]
pub(super) struct GifSubframeRegion {
    pub source_width: u16,
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
}

struct ExactPalette {
    palette_lookup: FxHashMap<u32, u8>,
    palette: Vec<u8>,
    transparent_index: Option<u8>,
    #[cfg(feature = "tracing")]
    unique_colors: usize,
}

#[cfg(test)]
pub(super) fn encode_rgba_frame(
    width: u16,
    height: u16,
    rgba: &mut [u8],
    quantizer_speed: i32,
) -> GifFrame<'static> {
    encode_rgba_subframe(
        GifSubframeRegion {
            source_width: width,
            left: 0,
            top: 0,
            width,
            height,
        },
        rgba,
        quantizer_speed,
    )
}

pub(super) fn encode_rgba_subframe(
    region: GifSubframeRegion,
    rgba: &mut [u8],
    quantizer_speed: i32,
) -> GifFrame<'static> {
    debug_assert_eq!(rgba.len() % (usize::from(region.source_width) * 4), 0);
    span_enter!(tracing::Level::TRACE, "gif_exact_palette");
    let transparent = canonicalize_transparent_pixels(rgba);
    if let Some(exact_palette) = build_exact_palette(rgba, transparent) {
        #[cfg(feature = "tracing")]
        trace!(
            unique_colors = exact_palette.unique_colors,
            "gif exact palette"
        );
        return GifFrame {
            width: region.width,
            height: region.height,
            buffer: Cow::Owned(crop_indexed_subframe(region, rgba, |pixel| {
                exact_palette
                    .palette_lookup
                    .get(&packed_rgba(pixel))
                    .copied()
                    .unwrap_or(0)
            })),
            palette: Some(exact_palette.palette),
            transparent: exact_palette.transparent_index,
            ..GifFrame::default()
        };
    }

    span_enter!(
        tracing::Level::TRACE,
        "gif_quantizer_fallback",
        quantizer_speed = quantizer_speed
    );
    let quantizer = NeuQuant::new(quantizer_speed, 256, rgba);
    GifFrame {
        width: region.width,
        height: region.height,
        buffer: Cow::Owned(crop_indexed_subframe(region, rgba, |pixel| {
            quantizer.index_of(pixel) as u8
        })),
        palette: Some(quantizer.color_map_rgb()),
        transparent: transparent.map(|color| quantizer.index_of(&color.to_be_bytes()) as u8),
        ..GifFrame::default()
    }
}

fn canonicalize_transparent_pixels(rgba: &mut [u8]) -> Option<u32> {
    let mut transparent: Option<u32> = None;

    for pixel in rgba.chunks_exact_mut(4) {
        if pixel[3] != 0 {
            pixel[3] = u8::MAX;
            continue;
        }

        if let Some(color) = transparent {
            pixel.copy_from_slice(&color.to_be_bytes());
        } else {
            transparent = Some(packed_rgba(pixel));
        }
    }

    transparent
}

fn build_exact_palette(rgba: &[u8], transparent: Option<u32>) -> Option<ExactPalette> {
    let mut colors = Vec::with_capacity(256);
    let mut seen = FxHashSet::default();
    seen.reserve(256);

    for pixel in rgba.chunks_exact(4) {
        let color = packed_rgba(pixel);
        if seen.insert(color) {
            if colors.len() == 256 {
                return None;
            }
            colors.push(color);
        }
    }

    colors.sort_unstable();
    let mut palette_lookup = FxHashMap::default();
    palette_lookup.reserve(colors.len());
    let mut palette = Vec::with_capacity(colors.len() * 3);
    for (index, color) in colors.iter().copied().enumerate() {
        palette_lookup.insert(color, index as u8);
        let [r, g, b, _a] = color.to_be_bytes();
        palette.extend([r, g, b]);
    }

    Some(ExactPalette {
        transparent_index: transparent.and_then(|color| palette_lookup.get(&color).copied()),
        palette_lookup,
        palette,
        #[cfg(feature = "tracing")]
        unique_colors: colors.len(),
    })
}

fn packed_rgba(pixel: &[u8]) -> u32 {
    u32::from_be_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])
}

fn crop_indexed_subframe<F>(region: GifSubframeRegion, rgba: &[u8], mut index_of: F) -> Vec<u8>
where
    F: FnMut(&[u8]) -> u8,
{
    let source_width = usize::from(region.source_width);
    let left = usize::from(region.left);
    let top = usize::from(region.top);
    let width = usize::from(region.width);
    let height = usize::from(region.height);
    let mut buffer = Vec::with_capacity(width * height);

    for y in top..top + height {
        let row_start = ((y * source_width) + left) * 4;
        let row_end = row_start + width * 4;
        for pixel in rgba[row_start..row_end].chunks_exact(4) {
            buffer.push(index_of(pixel));
        }
    }

    buffer
}

#[cfg(test)]
mod tests {
    use gif::Frame as GifFrame;

    use super::encode_rgba_frame;

    #[test]
    fn exact_palette_encoding_matches_gif_crate_for_small_palettes() {
        let mut pixels = vec![
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 10, 20, 30, 0, //
        ];
        let mut expected_pixels = pixels.clone();

        let encoded = encode_rgba_frame(2, 2, &mut pixels, 10);
        let expected = GifFrame::from_rgba_speed(2, 2, &mut expected_pixels, 10);

        assert_eq!(encoded.width, expected.width);
        assert_eq!(encoded.height, expected.height);
        assert_eq!(encoded.buffer, expected.buffer);
        assert_eq!(encoded.palette, expected.palette);
        assert_eq!(encoded.transparent, expected.transparent);
    }

    #[test]
    fn encoding_falls_back_to_gif_crate_for_large_palettes() {
        let mut pixels = Vec::with_capacity(17 * 16 * 4);
        for index in 0..272u16 {
            pixels.extend([
                (index & 0xFF) as u8,
                ((index * 3) & 0xFF) as u8,
                ((index * 5) & 0xFF) as u8,
                255,
            ]);
        }
        let mut expected_pixels = pixels.clone();

        let encoded = encode_rgba_frame(17, 16, &mut pixels, 7);
        let expected = GifFrame::from_rgba_speed(17, 16, &mut expected_pixels, 7);

        assert_eq!(encoded.buffer, expected.buffer);
        assert_eq!(encoded.palette, expected.palette);
        assert_eq!(encoded.transparent, expected.transparent);
    }
}
