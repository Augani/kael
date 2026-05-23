//! Raster page preview generation.

use std::sync::Arc;

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};

use crate::{
    annotation::{Annotation, PageAnnotation, PdfColor, PdfPoint, PdfRect},
    page::PdfPageSize,
};

/// A rasterized PDF page preview in RGBA pixel format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPage {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl RenderedPage {
    /// Creates a raster page from raw RGBA pixels.
    pub fn new(width: u32, height: u32, pixels: impl Into<Arc<[u8]>>) -> Self {
        Self {
            width,
            height,
            pixels: pixels.into(),
        }
    }

    /// Returns the rendered width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the rendered height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the RGBA pixel buffer.
    pub fn pixels(&self) -> &[u8] {
        self.pixels.as_ref()
    }
}

pub(crate) fn render_page_preview(
    page_size: PdfPageSize,
    text: &str,
    annotations: &[PageAnnotation],
    scale: f32,
) -> Result<RenderedPage> {
    let scale = scale.clamp(0.25, 4.0);
    let width = (page_size.width * scale).round().clamp(1.0, 4096.0) as u32;
    let height = (page_size.height * scale).round().clamp(1.0, 4096.0) as u32;
    let mut image = ImageBuffer::from_pixel(width, height, Rgba([255, 255, 255, 255]));

    draw_border(&mut image, Rgba([214, 219, 226, 255]));
    draw_text_bars(&mut image, text, scale);
    for annotation in annotations {
        draw_annotation(&mut image, page_size, annotation, scale);
    }

    Ok(RenderedPage::new(width, height, image.into_raw()))
}

fn draw_border(image: &mut RgbaImage, color: Rgba<u8>) {
    if image.width() == 0 || image.height() == 0 {
        return;
    }

    let max_x = image.width() - 1;
    let max_y = image.height() - 1;
    for x in 0..=max_x {
        image.put_pixel(x, 0, color);
        image.put_pixel(x, max_y, color);
    }
    for y in 0..=max_y {
        image.put_pixel(0, y, color);
        image.put_pixel(max_x, y, color);
    }
}

fn draw_text_bars(image: &mut RgbaImage, text: &str, scale: f32) {
    let margin = (24.0 * scale).round() as i32;
    let line_height = (10.0 * scale).round().max(6.0) as i32;
    let line_gap = (7.0 * scale).round().max(4.0) as i32;
    let mut y = margin;
    let available_width = image.width() as i32 - (margin * 2);

    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if y + line_height >= image.height() as i32 - margin {
            break;
        }

        let width_factor = (line.chars().count().clamp(1, 100) as f32) / 100.0;
        let bar_width = ((available_width as f32) * width_factor.max(0.12)).round() as i32;
        fill_rect(
            image,
            margin,
            y,
            bar_width.max((32.0 * scale) as i32),
            line_height,
            Rgba([88, 98, 118, 255]),
        );
        y += line_height + line_gap;
    }
}

fn draw_annotation(
    image: &mut RgbaImage,
    page_size: PdfPageSize,
    annotation: &PageAnnotation,
    scale: f32,
) {
    match &annotation.kind {
        Annotation::Highlight { rects, color } => {
            for rect in rects {
                let (x, y, width, height) = map_rect(page_size, *rect, scale);
                fill_rect(image, x, y, width, height, rgba(*color));
            }
        }
        Annotation::Note { position, .. } => {
            let x = (position.x * scale).round() as i32;
            let y = ((page_size.height - position.y) * scale).round() as i32;
            fill_rect(image, x - 5, y - 5, 10, 10, Rgba([255, 204, 64, 255]));
        }
        Annotation::FreeText { bounds, .. } => {
            let (x, y, width, height) = map_rect(page_size, *bounds, scale);
            stroke_rect(image, x, y, width, height, Rgba([56, 92, 158, 255]));
        }
        Annotation::Ink {
            paths,
            color,
            width,
        } => {
            let stroke_width = ((*width * scale).round() as i32).max(1);
            for path in paths {
                for segment in path.windows(2) {
                    if let [from, to] = segment {
                        draw_line(image, page_size, *from, *to, scale, rgba(*color), stroke_width);
                    }
                }
            }
        }
        Annotation::Stamp { bounds, .. } => {
            let (x, y, width, height) = map_rect(page_size, *bounds, scale);
            fill_rect(image, x, y, width, height, Rgba([222, 74, 74, 200]));
            stroke_rect(image, x, y, width, height, Rgba([160, 34, 34, 255]));
        }
    }
}

fn map_rect(page_size: PdfPageSize, rect: PdfRect, scale: f32) -> (i32, i32, i32, i32) {
    let x = (rect.x * scale).round() as i32;
    let y = ((page_size.height - rect.y - rect.height) * scale).round() as i32;
    let width = (rect.width * scale).round().max(1.0) as i32;
    let height = (rect.height * scale).round().max(1.0) as i32;
    (x, y, width, height)
}

fn rgba(color: PdfColor) -> Rgba<u8> {
    Rgba([color.red, color.green, color.blue, color.alpha])
}

fn fill_rect(image: &mut RgbaImage, x: i32, y: i32, width: i32, height: i32, color: Rgba<u8>) {
    for dy in 0..height.max(0) {
        for dx in 0..width.max(0) {
            blend_pixel(image, x + dx, y + dy, color);
        }
    }
}

fn stroke_rect(image: &mut RgbaImage, x: i32, y: i32, width: i32, height: i32, color: Rgba<u8>) {
    for dx in 0..width.max(0) {
        blend_pixel(image, x + dx, y, color);
        blend_pixel(image, x + dx, y + height.saturating_sub(1), color);
    }
    for dy in 0..height.max(0) {
        blend_pixel(image, x, y + dy, color);
        blend_pixel(image, x + width.saturating_sub(1), y + dy, color);
    }
}

fn draw_line(
    image: &mut RgbaImage,
    page_size: PdfPageSize,
    from: PdfPoint,
    to: PdfPoint,
    scale: f32,
    color: Rgba<u8>,
    stroke_width: i32,
) {
    let mut x0 = (from.x * scale).round() as i32;
    let mut y0 = ((page_size.height - from.y) * scale).round() as i32;
    let x1 = (to.x * scale).round() as i32;
    let y1 = ((page_size.height - to.y) * scale).round() as i32;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        fill_rect(
            image,
            x0 - stroke_width / 2,
            y0 - stroke_width / 2,
            stroke_width,
            stroke_width,
            color,
        );
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice_err = err * 2;
        if twice_err >= dy {
            err += dy;
            x0 += sx;
        }
        if twice_err <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn blend_pixel(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
        return;
    }

    let pixel = image.get_pixel_mut(x as u32, y as u32);
    let alpha = color[3] as f32 / 255.0;
    let inverse_alpha = 1.0 - alpha;
    pixel[0] = ((color[0] as f32 * alpha) + (pixel[0] as f32 * inverse_alpha)).round() as u8;
    pixel[1] = ((color[1] as f32 * alpha) + (pixel[1] as f32 * inverse_alpha)).round() as u8;
    pixel[2] = ((color[2] as f32 * alpha) + (pixel[2] as f32 * inverse_alpha)).round() as u8;
    pixel[3] = 255;
}