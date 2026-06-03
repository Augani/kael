//! Procedural source generators — solid color, linear gradient, checkerboard — usable
//! as timeline clips.
//!
//! These are the title / background / matte / test-pattern sources a real frame can be
//! produced from without any decoder. A clip references a generator through its `source`
//! string (e.g. `"color:0,1,0,1"`), and [`GeneratorProvider`] turns that into pixels for
//! the [`crate::compositor::composite_frame`] path.

use kael_render_graph::reference::Image;

use crate::compositor::FrameProvider;

/// Fill an image with a solid color.
pub fn solid(color: [f32; 4], width: u32, height: u32) -> Image {
    Image::filled(width, height, color)
}

/// A two-color linear gradient along the horizontal (default) or vertical axis.
pub fn linear_gradient(
    start: [f32; 4],
    end: [f32; 4],
    vertical: bool,
    width: u32,
    height: u32,
) -> Image {
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let t = if vertical {
                axis_fraction(y, height)
            } else {
                axis_fraction(x, width)
            };
            let mut color = [0.0f32; 4];
            for channel in 0..4 {
                color[channel] = start[channel] * (1.0 - t) + end[channel] * t;
            }
            image.pixels[(y * width + x) as usize] = color;
        }
    }
    image
}

fn axis_fraction(position: u32, extent: u32) -> f32 {
    if extent <= 1 {
        0.0
    } else {
        position as f32 / (extent - 1) as f32
    }
}

/// A two-color checkerboard test pattern with `cell`-pixel squares.
pub fn checkerboard(a: [f32; 4], b: [f32; 4], cell: u32, width: u32, height: u32) -> Image {
    let cell = cell.max(1);
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let even = ((x / cell) + (y / cell)).is_multiple_of(2);
            image.pixels[(y * width + x) as usize] = if even { a } else { b };
        }
    }
    image
}

/// A two-color radial gradient from `center` (at the image center) to `edge` (at the
/// farthest corner).
pub fn radial_gradient(center: [f32; 4], edge: [f32; 4], width: u32, height: u32) -> Image {
    let cx = (width as f32 - 1.0) / 2.0;
    let cy = (height as f32 - 1.0) / 2.0;
    let max_distance = (cx * cx + cy * cy).sqrt().max(f32::EPSILON);
    let mut image = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let t = ((dx * dx + dy * dy).sqrt() / max_distance).min(1.0);
            let mut color = [0.0f32; 4];
            for channel in 0..4 {
                color[channel] = center[channel] * (1.0 - t) + edge[channel] * t;
            }
            image.pixels[(y * width + x) as usize] = color;
        }
    }
    image
}

/// A procedural source: the parsed form of a generator clip's `source` string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Generator {
    /// A solid color fill.
    Solid([f32; 4]),
    /// A two-color linear gradient.
    LinearGradient {
        /// Color at the start of the axis.
        start: [f32; 4],
        /// Color at the end of the axis.
        end: [f32; 4],
        /// Gradient runs top-to-bottom when true, left-to-right when false.
        vertical: bool,
    },
    /// A two-color checkerboard.
    Checkerboard {
        /// Color of the even cells.
        a: [f32; 4],
        /// Color of the odd cells.
        b: [f32; 4],
        /// Square edge length in pixels.
        cell: u32,
    },
    /// A two-color radial gradient (center to edge).
    RadialGradient {
        /// Color at the image center.
        center: [f32; 4],
        /// Color at the farthest corner.
        edge: [f32; 4],
    },
}

impl Generator {
    /// Parse a generator from a clip `source` string, or `None` if it is not a
    /// recognized generator URI. Forms:
    /// * `color:r,g,b,a`
    /// * `gradient:r,g,b,a|r,g,b,a|h` (or `|v` for vertical)
    /// * `checker:r,g,b,a|r,g,b,a|cell`
    pub fn parse(source: &str) -> Option<Self> {
        let (scheme, rest) = source.split_once(':')?;
        match scheme {
            "color" => Some(Self::Solid(
                parse_hex_color(rest).or_else(|| parse_color(rest))?,
            )),
            "gradient" => {
                let parts: Vec<&str> = rest.split('|').collect();
                if parts.len() != 3 {
                    return None;
                }
                Some(Self::LinearGradient {
                    start: parse_color(parts[0])?,
                    end: parse_color(parts[1])?,
                    vertical: parts[2].trim() == "v",
                })
            }
            "checker" => {
                let parts: Vec<&str> = rest.split('|').collect();
                if parts.len() != 3 {
                    return None;
                }
                Some(Self::Checkerboard {
                    a: parse_color(parts[0])?,
                    b: parse_color(parts[1])?,
                    cell: parts[2].trim().parse::<u32>().ok()?,
                })
            }
            "radial" => {
                let parts: Vec<&str> = rest.split('|').collect();
                if parts.len() != 2 {
                    return None;
                }
                Some(Self::RadialGradient {
                    center: parse_color(parts[0])?,
                    edge: parse_color(parts[1])?,
                })
            }
            _ => None,
        }
    }

    /// Render this generator to a `width` x `height` image.
    pub fn render(&self, width: u32, height: u32) -> Image {
        match self {
            Self::Solid(color) => solid(*color, width, height),
            Self::LinearGradient {
                start,
                end,
                vertical,
            } => linear_gradient(*start, *end, *vertical, width, height),
            Self::Checkerboard { a, b, cell } => checkerboard(*a, *b, *cell, width, height),
            Self::RadialGradient { center, edge } => radial_gradient(*center, *edge, width, height),
        }
    }
}

fn parse_color(text: &str) -> Option<[f32; 4]> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut color = [0.0f32; 4];
    for (channel, part) in parts.iter().enumerate() {
        color[channel] = part.trim().parse::<f32>().ok()?;
    }
    Some(color)
}

/// Parse a hex color string (`#RGB`, `#RRGGBB`, or `#RRGGBBAA`) into `[r, g, b, a]` in
/// `0..=1` (missing alpha is opaque). The values are used straight, not sRGB-decoded.
/// Returns `None` if the string is not `#` followed by 3, 6, or 8 hex digits.
pub fn parse_hex_color(text: &str) -> Option<[f32; 4]> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |slice: &str| u8::from_str_radix(slice, 16).ok().map(|v| v as f32 / 255.0);
    match hex.len() {
        3 => {
            let mut color = [1.0f32; 4];
            for (index, digit) in hex.chars().enumerate() {
                let value = digit.to_digit(16)? as u8;
                color[index] = (value * 16 + value) as f32 / 255.0;
            }
            Some(color)
        }
        6 => Some([
            channel(&hex[0..2])?,
            channel(&hex[2..4])?,
            channel(&hex[4..6])?,
            1.0,
        ]),
        8 => Some([
            channel(&hex[0..2])?,
            channel(&hex[2..4])?,
            channel(&hex[4..6])?,
            channel(&hex[6..8])?,
        ]),
        _ => None,
    }
}

/// A [`FrameProvider`] that renders procedural generator clips from their `source` URI.
///
/// Sources that are not recognized generators yield `None`, so this can be the
/// generator half of a provider chain whose other half is a media decoder.
pub struct GeneratorProvider;

impl FrameProvider for GeneratorProvider {
    fn frame(&self, source: &str, _source_frame: u64, width: u32, height: u32) -> Option<Image> {
        Generator::parse(source).map(|generator| generator.render(width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::composite_frame;
    use crate::media::{Timeline, TimelineClip, TimelineTrack, TrackType};

    #[test]
    fn solid_fills_uniformly() {
        let image = solid([0.2, 0.4, 0.6, 1.0], 3, 2);
        assert!(image
            .pixels
            .iter()
            .all(|pixel| *pixel == [0.2, 0.4, 0.6, 1.0]));
    }

    #[test]
    fn horizontal_gradient_interpolates_across_x() {
        let image = linear_gradient([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0], false, 3, 1);
        assert_eq!(image.pixel(0, 0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(image.pixel(2, 0), [1.0, 1.0, 1.0, 1.0]);
        let mid = image.pixel(1, 0);
        assert!((mid[0] - 0.5).abs() < 1e-6, "midpoint {mid:?}");
    }

    #[test]
    fn checkerboard_alternates_cells() {
        let a = [1.0, 1.0, 1.0, 1.0];
        let b = [0.0, 0.0, 0.0, 1.0];
        let image = checkerboard(a, b, 1, 2, 2);
        assert_eq!(image.pixel(0, 0), a);
        assert_eq!(image.pixel(1, 0), b);
        assert_eq!(image.pixel(0, 1), b);
        assert_eq!(image.pixel(1, 1), a);
    }

    #[test]
    fn radial_gradient_runs_center_to_corner() {
        // 3x3: the center pixel is the center color, the corners are the edge color.
        let image = radial_gradient([1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0], 3, 3);
        assert_eq!(image.pixel(1, 1), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(image.pixel(0, 0), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn radial_generator_parses_and_renders() {
        assert_eq!(
            Generator::parse("radial:1,1,1,1|0,0,0,1"),
            Some(Generator::RadialGradient {
                center: [1.0; 4],
                edge: [0.0, 0.0, 0.0, 1.0]
            })
        );
        let rendered = Generator::parse("radial:1,1,1,1|0,0,0,1")
            .unwrap()
            .render(3, 3);
        assert_eq!(rendered.pixel(1, 1)[0], 1.0);
        // Missing the second color is rejected.
        assert_eq!(Generator::parse("radial:1,1,1,1"), None);
    }

    #[test]
    fn parses_generator_uris() {
        assert_eq!(
            Generator::parse("color:1,0,0,1"),
            Some(Generator::Solid([1.0, 0.0, 0.0, 1.0]))
        );
        assert!(matches!(
            Generator::parse("gradient:0,0,0,1|1,1,1,1|v"),
            Some(Generator::LinearGradient { vertical: true, .. })
        ));
        assert!(matches!(
            Generator::parse("checker:0,0,0,1|1,1,1,1|8"),
            Some(Generator::Checkerboard { cell: 8, .. })
        ));
        assert_eq!(Generator::parse("file.mp4"), None);
        assert_eq!(Generator::parse("color:1,0,0"), None);
        assert_eq!(Generator::parse("gradient:bad"), None);
    }

    #[test]
    fn parse_hex_color_handles_common_forms() {
        assert_eq!(parse_hex_color("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_hex_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        // #abc expands each nibble: a -> 0xaa.
        assert!((parse_hex_color("#abc").unwrap()[0] - 0xaa as f32 / 255.0).abs() < 1e-6);
        // 8-digit form carries alpha.
        let rgba = parse_hex_color("#00ff0080").unwrap();
        assert_eq!(rgba[1], 1.0);
        assert!((rgba[3] - 128.0 / 255.0).abs() < 1e-6);
        // Malformed.
        assert_eq!(parse_hex_color("ff0000"), None);
        assert_eq!(parse_hex_color("#ff00"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
    }

    #[test]
    fn color_generator_accepts_hex_and_floats() {
        assert_eq!(
            Generator::parse("color:#ff0000"),
            Some(Generator::Solid([1.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(
            Generator::parse("color:0,1,0,1"),
            Some(Generator::Solid([0.0, 1.0, 0.0, 1.0]))
        );
    }

    #[test]
    fn generator_clip_composites_through_the_full_path() {
        let timeline = Timeline {
            tracks: vec![TimelineTrack {
                id: "v1".to_string(),
                name: "v1".to_string(),
                track_type: TrackType::Video,
                enabled: true,
                clips: vec![TimelineClip {
                    id: "title".to_string(),
                    source: "color:0,1,0,1".to_string(),
                    start_frame: 0,
                    end_frame: 30,
                    track_offset: 0,
                    opacity: 1.0,
                    blend_mode: Default::default(),
                }],
            }],
            frame_rate: 30.0,
            duration_frames: 30,
        };
        let out = composite_frame(&timeline, 10, 2, 2, &GeneratorProvider);
        assert!(out
            .pixels
            .iter()
            .all(|pixel| *pixel == [0.0, 1.0, 0.0, 1.0]));
    }
}
