//! Schematic raster page preview generation.

use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::{
    annotation::{Annotation, PageAnnotation, PdfColor, PdfPoint, PdfRect},
    page::PdfPageSize,
};

type Rgba = [u8; 4];
pub(crate) const MAX_PREVIEW_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_PREVIEW_DRAW_OPERATIONS: usize = 16 * 1024 * 1024;
const MAX_RENDER_DIMENSION: u32 = 4_096;

struct DrawBudget {
    remaining: usize,
}

impl DrawBudget {
    const fn new(remaining: usize) -> Self {
        Self { remaining }
    }

    fn consume(&mut self) -> bool {
        let Some(remaining) = self.remaining.checked_sub(1) else {
            return false;
        };
        self.remaining = remaining;
        true
    }

    const fn exhausted(&self) -> bool {
        self.remaining == 0
    }
}

struct PreviewImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl PreviewImage {
    fn new(width: u32, height: u32, color: Rgba) -> Result<Self> {
        let mut pixels = vec![0; rgba_buffer_len(width, height)?];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn put_pixel(&mut self, x: u32, y: u32, color: Rgba) {
        if let Some(pixel) = self.pixel_mut(x, y) {
            pixel.copy_from_slice(&color);
        }
    }

    fn pixel_mut(&mut self, x: u32, y: u32) -> Option<&mut [u8]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (u64::from(y) * u64::from(self.width) + u64::from(x)) * 4;
        let offset = usize::try_from(offset).ok()?;
        self.pixels.get_mut(offset..offset + 4)
    }

    fn into_raw(self) -> Vec<u8> {
        self.pixels
    }
}

/// A schematic PDF page preview in RGBA pixel format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagePreview {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl PagePreview {
    /// Creates a raster page from raw RGBA pixels.
    pub fn new(width: u32, height: u32, pixels: impl Into<Arc<[u8]>>) -> Result<Self> {
        let expected_len = rgba_buffer_len(width, height)?;
        let pixels = pixels.into();
        anyhow::ensure!(
            pixels.len() == expected_len,
            "rendered page has {} RGBA bytes; expected {expected_len}",
            pixels.len()
        );
        Ok(Self {
            width,
            height,
            pixels,
        })
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

/// A bounded LRU cache for schematic PDF page previews.
pub struct PagePreviewCache {
    max_pages: usize,
    max_bytes: usize,
    bytes: usize,
    pages: VecDeque<(usize, PagePreview)>,
}

impl PagePreviewCache {
    /// Creates a new cache with the given maximum number of retained pages.
    pub fn new(max_pages: usize) -> Self {
        Self::with_limits(max_pages, MAX_PREVIEW_CACHE_BYTES)
    }

    fn with_limits(max_pages: usize, max_bytes: usize) -> Self {
        Self {
            max_pages,
            max_bytes,
            bytes: 0,
            pages: VecDeque::new(),
        }
    }

    /// Returns a cheap clone of the preview at `page_index` and marks it recently used.
    pub fn get(&mut self, page_index: usize) -> Option<PagePreview> {
        let position = self
            .pages
            .iter()
            .position(|(index, _)| *index == page_index)?;
        let entry = self.pages.remove(position)?;
        let preview = entry.1.clone();
        self.pages.push_back(entry);
        Some(preview)
    }

    /// Inserts or replaces the rendered page for `page_index`.
    ///
    /// The oldest entries are evicted to stay within the configured page count and the crate's
    /// 128 MiB cache budget.
    pub fn insert(&mut self, page_index: usize, page: PagePreview) {
        if let Some(position) = self.pages.iter().position(|(idx, _)| *idx == page_index)
            && let Some((_, replaced)) = self.pages.remove(position)
        {
            self.bytes = self.bytes.saturating_sub(replaced.pixels().len());
        }
        let page_bytes = page.pixels().len();
        if self.max_pages == 0 || page_bytes > self.max_bytes {
            return;
        }
        while self.pages.len() >= self.max_pages
            || self.bytes.saturating_add(page_bytes) > self.max_bytes
        {
            let Some((_, evicted)) = self.pages.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.pixels().len());
        }
        self.bytes = self.bytes.saturating_add(page_bytes);
        self.pages.push_back((page_index, page));
    }

    /// Returns the number of cached pages.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Returns true if the cache contains no pages.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

pub(crate) fn render_schematic_preview(
    page_size: PdfPageSize,
    text: &str,
    annotations: &[PageAnnotation],
    scale: f32,
) -> Result<PagePreview> {
    let scale = normalize_scale(scale)?;
    anyhow::ensure!(
        page_size.width.is_finite()
            && page_size.height.is_finite()
            && page_size.width > 0.0
            && page_size.height > 0.0,
        "PDF page size must contain finite positive dimensions"
    );
    let (width, height, scale) = preview_geometry(page_size, scale);
    let mut image = PreviewImage::new(width, height, [255, 255, 255, 255])?;
    let mut budget = DrawBudget::new(MAX_PREVIEW_DRAW_OPERATIONS);

    draw_border(&mut image, [214, 219, 226, 255], &mut budget);
    draw_text_bars(&mut image, text, scale, &mut budget);
    for annotation in annotations {
        if budget.exhausted() {
            break;
        }
        draw_annotation(&mut image, page_size, annotation, scale, &mut budget);
    }

    PagePreview::new(width, height, image.into_raw())
}

pub(crate) fn normalize_scale(scale: f32) -> Result<f32> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(anyhow!(
            "PDF render scale must be finite and greater than zero"
        ));
    }
    Ok((scale.clamp(0.25, 4.0) * 1_000.0).round() / 1_000.0)
}

fn rgba_buffer_len(width: u32, height: u32) -> Result<usize> {
    anyhow::ensure!(
        width > 0 && height > 0,
        "rendered page dimensions must be greater than zero"
    );
    anyhow::ensure!(
        width <= MAX_RENDER_DIMENSION && height <= MAX_RENDER_DIMENSION,
        "rendered page dimensions exceed the {MAX_RENDER_DIMENSION} pixel limit"
    );
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| anyhow!("rendered page RGBA byte length overflow"))?;
    usize::try_from(pixels).map_err(|_| anyhow!("rendered page is too large for this platform"))
}

fn preview_geometry(page_size: PdfPageSize, requested_scale: f32) -> (u32, u32, f32) {
    let fitted_scale = requested_scale
        .min(MAX_RENDER_DIMENSION as f32 / page_size.width)
        .min(MAX_RENDER_DIMENSION as f32 / page_size.height);
    let width = (page_size.width * fitted_scale)
        .round()
        .clamp(1.0, MAX_RENDER_DIMENSION as f32) as u32;
    let height = (page_size.height * fitted_scale)
        .round()
        .clamp(1.0, MAX_RENDER_DIMENSION as f32) as u32;
    (width, height, fitted_scale)
}

fn draw_border(image: &mut PreviewImage, color: Rgba, budget: &mut DrawBudget) {
    if image.width() == 0 || image.height() == 0 {
        return;
    }

    let max_x = image.width() - 1;
    let max_y = image.height() - 1;
    for x in 0..=max_x {
        if !budget.consume() {
            return;
        }
        image.put_pixel(x, 0, color);
        if !budget.consume() {
            return;
        }
        image.put_pixel(x, max_y, color);
    }
    for y in 0..=max_y {
        if !budget.consume() {
            return;
        }
        image.put_pixel(0, y, color);
        if !budget.consume() {
            return;
        }
        image.put_pixel(max_x, y, color);
    }
}

fn draw_text_bars(image: &mut PreviewImage, text: &str, scale: f32, budget: &mut DrawBudget) {
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
            [88, 98, 118, 255],
            budget,
        );
        if budget.exhausted() {
            break;
        }
        y += line_height + line_gap;
    }
}

fn draw_annotation(
    image: &mut PreviewImage,
    page_size: PdfPageSize,
    annotation: &PageAnnotation,
    scale: f32,
    budget: &mut DrawBudget,
) {
    match &annotation.kind {
        Annotation::Highlight { rects, color } => {
            for rect in rects {
                let (x, y, width, height) = map_rect(page_size, *rect, scale);
                fill_rect(image, x, y, width, height, rgba(*color), budget);
                if budget.exhausted() {
                    break;
                }
            }
        }
        Annotation::Note { position, .. } => {
            let x = (position.x * scale).round() as i32;
            let y = ((page_size.height - position.y) * scale).round() as i32;
            fill_rect(image, x - 5, y - 5, 10, 10, [255, 204, 64, 255], budget);
        }
        Annotation::FreeText { bounds, .. } => {
            let (x, y, width, height) = map_rect(page_size, *bounds, scale);
            stroke_rect(image, x, y, width, height, [56, 92, 158, 255], budget);
        }
        Annotation::Ink {
            paths,
            color,
            width,
        } => {
            let max_stroke_width = image
                .width()
                .max(image.height())
                .saturating_mul(2)
                .try_into()
                .unwrap_or(i32::MAX);
            let stroke_width = ((*width * scale).round() as i32).clamp(1, max_stroke_width);
            for path in paths {
                for segment in path.windows(2) {
                    if budget.exhausted() {
                        return;
                    }
                    if let [from, to] = segment {
                        draw_line(
                            image,
                            page_size,
                            *from,
                            *to,
                            scale,
                            rgba(*color),
                            stroke_width,
                            budget,
                        );
                    }
                }
            }
        }
        Annotation::Stamp { bounds, .. } => {
            let (x, y, width, height) = map_rect(page_size, *bounds, scale);
            fill_rect(image, x, y, width, height, [222, 74, 74, 200], budget);
            stroke_rect(image, x, y, width, height, [160, 34, 34, 255], budget);
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

fn rgba(color: PdfColor) -> Rgba {
    [color.red, color.green, color.blue, color.alpha]
}

fn fill_rect(
    image: &mut PreviewImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: Rgba,
    budget: &mut DrawBudget,
) {
    if width <= 0 || height <= 0 || budget.exhausted() {
        return;
    }
    let image_width = i64::from(image.width());
    let image_height = i64::from(image.height());
    let start_x = i64::from(x).clamp(0, image_width);
    let start_y = i64::from(y).clamp(0, image_height);
    let end_x = i64::from(x)
        .saturating_add(i64::from(width))
        .clamp(0, image_width);
    let end_y = i64::from(y)
        .saturating_add(i64::from(height))
        .clamp(0, image_height);

    for pixel_y in start_y..end_y {
        for pixel_x in start_x..end_x {
            if !budget.consume() {
                return;
            }
            blend_pixel(image, pixel_x as i32, pixel_y as i32, color);
        }
    }
}

fn stroke_rect(
    image: &mut PreviewImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: Rgba,
    budget: &mut DrawBudget,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    fill_rect(image, x, y, width, 1, color, budget);
    fill_rect(
        image,
        x,
        y.saturating_add(height.saturating_sub(1)),
        width,
        1,
        color,
        budget,
    );
    fill_rect(image, x, y, 1, height, color, budget);
    fill_rect(
        image,
        x.saturating_add(width.saturating_sub(1)),
        y,
        1,
        height,
        color,
        budget,
    );
}

fn draw_line(
    image: &mut PreviewImage,
    page_size: PdfPageSize,
    from: PdfPoint,
    to: PdfPoint,
    scale: f32,
    color: Rgba,
    stroke_width: i32,
    budget: &mut DrawBudget,
) {
    let x0 = (from.x * scale).round() as i32;
    let y0 = ((page_size.height - from.y) * scale).round() as i32;
    let x1 = (to.x * scale).round() as i32;
    let y1 = ((page_size.height - to.y) * scale).round() as i32;
    let padding = stroke_width / 2;
    let Some((mut x0, mut y0, x1, y1)) = clip_line_to_image(image, x0, y0, x1, y1, padding) else {
        return;
    };
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
            budget,
        );
        if budget.exhausted() {
            return;
        }
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

fn clip_line_to_image(
    image: &PreviewImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    padding: i32,
) -> Option<(i32, i32, i32, i32)> {
    let min_x = -f64::from(padding);
    let min_y = -f64::from(padding);
    let max_x = f64::from(image.width().saturating_sub(1)) + f64::from(padding);
    let max_y = f64::from(image.height().saturating_sub(1)) + f64::from(padding);
    let x0 = f64::from(x0);
    let y0 = f64::from(y0);
    let dx = f64::from(x1) - x0;
    let dy = f64::from(y1) - y0;
    let mut start = 0.0f64;
    let mut end = 1.0f64;

    for (direction, distance) in [
        (-dx, x0 - min_x),
        (dx, max_x - x0),
        (-dy, y0 - min_y),
        (dy, max_y - y0),
    ] {
        if direction == 0.0 {
            if distance < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = distance / direction;
        if direction < 0.0 {
            start = start.max(ratio);
        } else {
            end = end.min(ratio);
        }
        if start > end {
            return None;
        }
    }

    Some((
        (x0 + start * dx).round() as i32,
        (y0 + start * dy).round() as i32,
        (x0 + end * dx).round() as i32,
        (y0 + end * dy).round() as i32,
    ))
}

fn blend_pixel(image: &mut PreviewImage, x: i32, y: i32, color: Rgba) {
    if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
        return;
    }

    let Some(pixel) = image.pixel_mut(x as u32, y as u32) else {
        return;
    };
    let alpha = color[3] as f32 / 255.0;
    let inverse_alpha = 1.0 - alpha;
    pixel[0] = ((color[0] as f32 * alpha) + (pixel[0] as f32 * inverse_alpha)).round() as u8;
    pixel[1] = ((color[1] as f32 * alpha) + (pixel[1] as f32 * inverse_alpha)).round() as u8;
    pixel[2] = ((color[2] as f32 * alpha) + (pixel[2] as f32 * inverse_alpha)).round() as u8;
    pixel[3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_cache_respects_limit() {
        let mut cache = PagePreviewCache::new(3);
        for i in 0..4 {
            cache.insert(
                i,
                PagePreview::new(100, 100, Arc::<[u8]>::from(vec![0u8; 40000])).unwrap(),
            );
        }
        assert!(cache.get(0).is_none());
        assert!(cache.get(3).is_some());
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn page_cache_refreshes_recently_used_entries() {
        let mut cache = PagePreviewCache::new(2);
        let preview = || PagePreview::new(1, 1, Arc::<[u8]>::from([0, 0, 0, 0])).unwrap();
        cache.insert(0, preview());
        cache.insert(1, preview());

        assert!(cache.get(0).is_some());
        cache.insert(2, preview());

        assert!(cache.get(0).is_some());
        assert!(cache.get(1).is_none());
        assert!(cache.get(2).is_some());
    }

    #[test]
    fn page_cache_overwrites_existing_entry() {
        let mut cache = PagePreviewCache::new(3);
        cache.insert(
            0,
            PagePreview::new(100, 100, Arc::<[u8]>::from(vec![1u8; 40000])).unwrap(),
        );
        cache.insert(
            0,
            PagePreview::new(200, 200, Arc::<[u8]>::from(vec![2u8; 160000])).unwrap(),
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(0).unwrap().width(), 200);
    }

    #[test]
    fn page_cache_is_empty_on_new() {
        let cache = PagePreviewCache::new(5);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn zero_capacity_cache_never_retains_pages() {
        let mut cache = PagePreviewCache::new(0);
        cache.insert(
            0,
            PagePreview::new(1, 1, Arc::<[u8]>::from([0, 0, 0, 0])).unwrap(),
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn page_cache_respects_its_byte_limit() {
        let mut cache = PagePreviewCache::with_limits(3, 8);
        let preview = || PagePreview::new(1, 1, Arc::<[u8]>::from([0, 0, 0, 0])).unwrap();
        cache.insert(0, preview());
        cache.insert(1, preview());
        cache.insert(2, preview());

        assert_eq!(cache.len(), 2);
        assert!(cache.get(0).is_none());
        assert!(cache.get(1).is_some());
        assert!(cache.get(2).is_some());
    }

    #[test]
    fn page_previews_validate_dimensions_and_rgba_length() {
        assert!(PagePreview::new(0, 1, Arc::<[u8]>::from([])).is_err());
        assert!(PagePreview::new(1, 1, Arc::<[u8]>::from([0, 0, 0])).is_err());
        assert!(PagePreview::new(4_097, 1, Arc::<[u8]>::from([])).is_err());
        assert!(normalize_scale(f32::NAN).is_err());
        assert!(normalize_scale(f32::INFINITY).is_err());
        assert!(normalize_scale(0.0).is_err());
        assert_eq!(normalize_scale(0.1).unwrap(), 0.25);
        assert_eq!(normalize_scale(9.0).unwrap(), 4.0);
    }

    #[test]
    fn oversized_pages_preserve_their_aspect_ratio() {
        let (width, height, scale) = preview_geometry(PdfPageSize::new(20_000.0, 1_000.0), 4.0);

        assert_eq!((width, height), (4_096, 205));
        assert!((scale - 0.2048).abs() < f32::EPSILON);
    }

    #[test]
    fn drawing_clips_untrusted_geometry_and_respects_the_work_budget() {
        let mut image = PreviewImage::new(8, 8, [255, 255, 255, 255]).unwrap();
        let mut budget = DrawBudget::new(4);

        fill_rect(
            &mut image,
            -10_000_000,
            -10_000_000,
            20_000_000,
            20_000_000,
            [0, 0, 0, 255],
            &mut budget,
        );

        assert!(budget.exhausted());
        assert_eq!(
            image
                .pixels
                .chunks_exact(4)
                .filter(|pixel| *pixel == [0, 0, 0, 255])
                .count(),
            4
        );

        assert!(
            clip_line_to_image(&image, -10_000_000, -10_000_000, -9_000_000, -9_000_000, 1,)
                .is_none()
        );
        assert_eq!(
            clip_line_to_image(&image, -10, 4, 10, 4, 0),
            Some((0, 4, 7, 4))
        );
    }
}
