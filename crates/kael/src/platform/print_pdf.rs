//! Bounded rendering of Kael print commands into a portable PDF.
//!
//! Pages are rendered through the same `resvg`/system-font path available to
//! every native backend and embedded as losslessly-compressed RGB page images.
//! This makes the platform spoolers consume identical content, including
//! rounded geometry, Unicode text, image fitting, clipping, alpha, and opacity.
//! Output is reproducible for a fixed installed font set; system-font fallback
//! can intentionally differ when the same requested family is absent on a host.

use crate::{
    Bounds, Edges, Pixels, Point, PrintImageFit, RenderImage, Rgba, SvgRenderer, SvgSize,
    platform::PlatformPrintJob,
    print::{PrintCommand, PrintTextStyle, oriented_print_page_size},
};
use anyhow::{Context as _, Result, anyhow, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
use image::RgbImage;
use image::{DynamicImage, ImageFormat, RgbaImage};
use std::{collections::HashMap, fmt::Write as _, io::Cursor, sync::Arc};

const MAX_PRINT_PAGES: usize = 256;
const MAX_PRINT_COMMANDS: usize = 200_000;
const MAX_PRINT_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PRINT_SVG_BYTES: usize = 128 * 1024 * 1024;
const MAX_PRINT_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_PRINT_TOTAL_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_PRINT_IMAGE_DATA_URL_BYTES: usize = 64 * 1024 * 1024;
const MAX_PRINT_TOTAL_IMAGE_DATA_URL_BYTES: usize = 96 * 1024 * 1024;
const MAX_PRINT_PAGE_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_PRINT_TOTAL_PAGE_PIXELS: u64 = 128 * 1024 * 1024;
#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
const MAX_PDF_BYTES: usize = 256 * 1024 * 1024;
const IDEAL_RASTER_SCALE: f64 = 2.0;
const MIN_RASTER_SCALE: f64 = 1.0;

/// Render a checked native print job to bounded PDF bytes.
#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
pub(crate) fn render_print_job_pdf(job: &PlatformPrintJob) -> Result<Vec<u8>> {
    let (page_width, page_height) = oriented_page_size(job);
    let rasters = render_print_job_pages(job)?;
    let mut page_images = Vec::new();
    page_images
        .try_reserve_exact(rasters.len())
        .context("allocating PDF page descriptor table")?;
    for raster in &rasters {
        page_images.push(png_predictor_stream_from_raster(raster)?);
    }
    write_pdf(job.title.as_ref(), page_width, page_height, &page_images)
}

pub(crate) struct PrintPageRaster {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Opaque BGRA pixels in top-down row order.
    pub(crate) bgra: Vec<u8>,
}

/// Render every page using the same bounded portable path used by PDF output.
/// Windows sends these top-down pages directly to the selected printer HDC.
pub(crate) fn render_print_job_pages(job: &PlatformPrintJob) -> Result<Vec<PrintPageRaster>> {
    let limits = validate_job(job)?;
    let scale = choose_raster_scale(limits.base_page_pixels)?;
    let renderer = SvgRenderer::new(Arc::new(()));
    let mut image_cache = HashMap::new();
    let mut image_budget = ImageRenderBudget::default();
    let mut page_rasters = Vec::new();
    page_rasters
        .try_reserve_exact(job.pages.len())
        .context("allocating print page raster table")?;

    let (page_width, page_height) = oriented_page_size(job);
    let mut total_page_pixels = 0u64;
    for (page_index, page) in job.pages.iter().enumerate() {
        let svg = render_page_svg(
            page_index,
            page.commands.as_slice(),
            page_width,
            page_height,
            job.margins,
            &mut image_cache,
            &mut image_budget,
        )?;
        ensure!(
            svg.len() <= MAX_PRINT_SVG_BYTES,
            "print page SVG exceeded the {MAX_PRINT_SVG_BYTES}-byte safety limit"
        );
        let pixmap = renderer
            .render_pixmap(svg.as_bytes(), SvgSize::ScaleFactor(scale as f32))
            .map_err(|error| anyhow!("rendering print page {}: {error}", page_index + 1))?;
        let page_pixels = u64::from(pixmap.width())
            .checked_mul(u64::from(pixmap.height()))
            .context("print page raster dimensions overflowed")?;
        ensure!(
            page_pixels <= MAX_PRINT_PAGE_PIXELS,
            "print page raster exceeded the {MAX_PRINT_PAGE_PIXELS}-pixel safety limit"
        );
        total_page_pixels = total_page_pixels
            .checked_add(page_pixels)
            .context("total print page pixel count overflowed")?;
        ensure!(
            total_page_pixels <= MAX_PRINT_TOTAL_PAGE_PIXELS,
            "print job raster exceeded the {MAX_PRINT_TOTAL_PAGE_PIXELS}-pixel safety limit"
        );
        page_rasters.push(page_raster_from_pixmap(&pixmap)?);
    }
    Ok(page_rasters)
}

struct ValidatedJob {
    base_page_pixels: u64,
}

fn validate_job(job: &PlatformPrintJob) -> Result<ValidatedJob> {
    ensure!(
        !job.pages.is_empty(),
        "print jobs must contain at least one page"
    );
    ensure!(
        job.pages.len() <= MAX_PRINT_PAGES,
        "print jobs cannot exceed {MAX_PRINT_PAGES} pages"
    );
    let (width, height) = oriented_page_size(job);
    ensure!(
        width.is_finite() && height.is_finite() && width >= 1.0 && height >= 1.0,
        "print page dimensions must be finite and positive"
    );
    ensure!(
        width <= 16_384.0 && height <= 16_384.0,
        "print page dimensions exceed the portable renderer limit"
    );
    let base_pixels_per_page = (width.ceil() as u64)
        .checked_mul(height.ceil() as u64)
        .context("print page dimensions overflowed")?;
    ensure!(
        base_pixels_per_page <= MAX_PRINT_PAGE_PIXELS,
        "print page exceeds the {MAX_PRINT_PAGE_PIXELS}-pixel safety limit at 72 DPI"
    );
    let base_page_pixels = base_pixels_per_page
        .checked_mul(job.pages.len() as u64)
        .context("print job page pixel count overflowed")?;
    ensure!(
        base_page_pixels <= MAX_PRINT_TOTAL_PAGE_PIXELS,
        "print job exceeds the {MAX_PRINT_TOTAL_PAGE_PIXELS}-pixel safety limit at 72 DPI"
    );

    let mut command_count = 0usize;
    let mut text_bytes = 0usize;
    for page in &job.pages {
        command_count = command_count
            .checked_add(page.commands.len())
            .context("print command count overflowed")?;
        for command in &page.commands {
            if let PrintCommand::Text { text, .. } | PrintCommand::TextBlock { text, .. } = command
            {
                text_bytes = text_bytes
                    .checked_add(text.len())
                    .context("print text byte count overflowed")?;
            }
        }
    }
    ensure!(
        command_count <= MAX_PRINT_COMMANDS,
        "print jobs cannot exceed {MAX_PRINT_COMMANDS} commands"
    );
    ensure!(
        text_bytes <= MAX_PRINT_TEXT_BYTES,
        "print jobs cannot exceed {MAX_PRINT_TEXT_BYTES} text bytes"
    );
    Ok(ValidatedJob { base_page_pixels })
}

fn choose_raster_scale(base_page_pixels: u64) -> Result<f64> {
    ensure!(base_page_pixels > 0, "print job raster would be empty");
    let budget_scale = ((MAX_PRINT_TOTAL_PAGE_PIXELS as f64) / (base_page_pixels as f64)).sqrt();
    let scale = IDEAL_RASTER_SCALE.min(budget_scale).max(MIN_RASTER_SCALE);
    ensure!(scale.is_finite(), "print job raster scale is invalid");
    Ok(scale)
}

fn oriented_page_size(job: &PlatformPrintJob) -> (f64, f64) {
    let page_size = oriented_print_page_size(job.page_size, job.orientation);
    (f64::from(page_size.width.0), f64::from(page_size.height.0))
}

#[allow(clippy::too_many_arguments)]
fn render_page_svg(
    page_index: usize,
    commands: &[PrintCommand],
    page_width: f64,
    page_height: f64,
    margins: Edges<Pixels>,
    image_cache: &mut HashMap<(crate::ImageId, usize), String>,
    image_budget: &mut ImageRenderBudget,
) -> Result<String> {
    let mut svg = String::with_capacity(commands.len().saturating_mul(160).min(8 * 1024 * 1024));
    write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{page_width}\" height=\"{page_height}\" viewBox=\"0 0 {page_width} {page_height}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>"
    )?;
    for (command_index, command) in commands.iter().enumerate() {
        write_print_command(
            &mut svg,
            command,
            margins,
            page_index,
            command_index,
            image_cache,
            image_budget,
        )?;
        ensure!(
            svg.len() <= MAX_PRINT_SVG_BYTES,
            "print page SVG exceeded the {MAX_PRINT_SVG_BYTES}-byte safety limit"
        );
    }
    svg.push_str("</svg>");
    Ok(svg)
}

#[allow(clippy::too_many_arguments)]
fn write_print_command(
    svg: &mut String,
    command: &PrintCommand,
    margins: Edges<Pixels>,
    page_index: usize,
    command_index: usize,
    image_cache: &mut HashMap<(crate::ImageId, usize), String>,
    image_budget: &mut ImageRenderBudget,
) -> Result<()> {
    match command {
        PrintCommand::FillRect { bounds, color } => {
            write_rect(svg, *bounds, margins, None, Some(*color), None)?;
        }
        PrintCommand::FillRoundedRect {
            bounds,
            radius,
            color,
        } => write_rect(svg, *bounds, margins, Some(*radius), Some(*color), None)?,
        PrintCommand::StrokeRect { bounds, stroke } => {
            write_rect(svg, *bounds, margins, None, None, Some(stroke))?;
        }
        PrintCommand::StrokeRoundedRect {
            bounds,
            radius,
            stroke,
        } => write_rect(svg, *bounds, margins, Some(*radius), None, Some(stroke))?,
        PrintCommand::StrokeLine { from, to, stroke } => {
            let color = stroke.color_ref();
            write!(
                svg,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"/>",
                (margins.left + from.x).0,
                (margins.top + from.y).0,
                (margins.left + to.x).0,
                (margins.top + to.y).0,
                color_hex(color),
                alpha(color),
                stroke.width().0,
            )?;
        }
        PrintCommand::Text {
            origin,
            text,
            style,
        } => write_text(svg, *origin, text.as_ref(), style, margins)?,
        PrintCommand::TextBlock {
            bounds,
            text,
            style,
        } => write_text_block(
            svg,
            *bounds,
            text.as_ref(),
            style,
            margins,
            page_index,
            command_index,
        )?,
        PrintCommand::Image {
            bounds,
            image,
            style,
        } => {
            let frame_index = style.selected_frame_index();
            let key = (image.id, frame_index);
            let data_url = match image_cache.entry(key) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(render_image_data_url(image, frame_index, image_budget)?)
                }
            };
            let fitted = fitted_image_bounds(*bounds, image, style.object_fit_ref(), frame_index)?;
            let clip_id = format!("p{page_index}c{command_index}i");
            let clip = offset_bounds(*bounds, margins);
            let fitted = offset_bounds(fitted, margins);
            write!(
                svg,
                "<defs><clipPath id=\"{clip_id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs><image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" opacity=\"{}\" preserveAspectRatio=\"none\" clip-path=\"url(#{clip_id})\" href=\"{}\"/>",
                clip.origin.x.0,
                clip.origin.y.0,
                clip.size.width.0,
                clip.size.height.0,
                fitted.origin.x.0,
                fitted.origin.y.0,
                fitted.size.width.0,
                fitted.size.height.0,
                style.opacity_ref(),
                data_url,
            )?;
        }
    }
    Ok(())
}

fn write_rect(
    svg: &mut String,
    bounds: Bounds<Pixels>,
    margins: Edges<Pixels>,
    radius: Option<Pixels>,
    fill: Option<Rgba>,
    stroke: Option<&crate::PrintStroke>,
) -> Result<()> {
    let bounds = offset_bounds(bounds, margins);
    write!(
        svg,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
        bounds.origin.x.0, bounds.origin.y.0, bounds.size.width.0, bounds.size.height.0,
    )?;
    if let Some(radius) = radius {
        write!(svg, " rx=\"{}\" ry=\"{}\"", radius.0, radius.0)?;
    }
    if let Some(fill) = fill {
        write!(
            svg,
            " fill=\"{}\" fill-opacity=\"{}\"",
            color_hex(fill),
            alpha(fill)
        )?;
    } else {
        svg.push_str(" fill=\"none\"");
    }
    if let Some(stroke) = stroke {
        write!(
            svg,
            " stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
            color_hex(stroke.color_ref()),
            alpha(stroke.color_ref()),
            stroke.width().0,
        )?;
    }
    svg.push_str("/>");
    Ok(())
}

fn write_text(
    svg: &mut String,
    origin: Point<Pixels>,
    text: &str,
    style: &PrintTextStyle,
    margins: Edges<Pixels>,
) -> Result<()> {
    let color = style.color_ref();
    write!(
        svg,
        "<text xml:space=\"preserve\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\" fill-opacity=\"{}\"{}>{}</text>",
        (margins.left + origin.x).0,
        (margins.top + origin.y + style.font_size()).0,
        style.font_size().0,
        color_hex(color),
        alpha(color),
        font_family_attribute(style),
        escape_xml(text),
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_text_block(
    svg: &mut String,
    bounds: Bounds<Pixels>,
    text: &str,
    style: &PrintTextStyle,
    margins: Edges<Pixels>,
    page_index: usize,
    command_index: usize,
) -> Result<()> {
    let bounds = offset_bounds(bounds, margins);
    let line_height = style.font_size().0 * 1.2;
    let max_lines = ((bounds.size.height.0 / line_height).ceil() as usize).max(1);
    let approximate_character_width = (style.font_size().0 * 0.55).max(1.0);
    let max_characters =
        ((bounds.size.width.0 / approximate_character_width).floor() as usize).max(1);
    let lines = wrap_text(text, max_characters, max_lines);
    let clip_id = format!("p{page_index}c{command_index}t");
    let color = style.color_ref();
    write!(
        svg,
        "<defs><clipPath id=\"{clip_id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs><text xml:space=\"preserve\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\" fill-opacity=\"{}\"{} clip-path=\"url(#{clip_id})\">",
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
        bounds.origin.x.0,
        bounds.origin.y.0 + style.font_size().0,
        style.font_size().0,
        color_hex(color),
        alpha(color),
        font_family_attribute(style),
    )?;
    for (line_index, line) in lines.iter().enumerate() {
        write!(
            svg,
            "<tspan x=\"{}\" dy=\"{}\">{}</tspan>",
            bounds.origin.x.0,
            if line_index == 0 { 0.0 } else { line_height },
            escape_xml(line),
        )?;
    }
    svg.push_str("</text>");
    Ok(())
}

fn wrap_text(text: &str, max_characters: usize, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.replace('\t', "    ").split('\n') {
        if lines.len() >= max_lines {
            break;
        }
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_chars = 0usize;
        for word in paragraph.split_whitespace() {
            let word_chars = word.chars().count();
            if !current.is_empty() && current_chars.saturating_add(1 + word_chars) > max_characters
            {
                lines.push(std::mem::take(&mut current));
                current_chars = 0;
                if lines.len() >= max_lines {
                    break;
                }
            }
            if word_chars > max_characters {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                let mut segment = String::new();
                for character in word.chars() {
                    segment.push(character);
                    if segment.chars().count() == max_characters {
                        lines.push(std::mem::take(&mut segment));
                        if lines.len() >= max_lines {
                            break;
                        }
                    }
                }
                if lines.len() >= max_lines {
                    break;
                }
                current_chars = segment.chars().count();
                current = segment;
            } else {
                if !current.is_empty() {
                    current.push(' ');
                    current_chars += 1;
                }
                current.push_str(word);
                current_chars += word_chars;
            }
        }
        if lines.len() < max_lines && !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn fitted_image_bounds(
    bounds: Bounds<Pixels>,
    image: &RenderImage,
    fit: PrintImageFit,
    frame_index: usize,
) -> Result<Bounds<Pixels>> {
    let image_size = image.size(frame_index);
    let image_width = u32::from(image_size.width) as f32;
    let image_height = u32::from(image_size.height) as f32;
    ensure!(
        image_width > 0.0 && image_height > 0.0,
        "print image dimensions must be positive"
    );
    let image_ratio = image_width / image_height;
    let bounds_ratio = bounds.size.width.0 / bounds.size.height.0;
    let fitted_size = match fit {
        PrintImageFit::Fill => bounds.size,
        PrintImageFit::Contain => {
            if bounds_ratio > image_ratio {
                crate::size(
                    Pixels(image_width * bounds.size.height.0 / image_height),
                    bounds.size.height,
                )
            } else {
                crate::size(
                    bounds.size.width,
                    Pixels(image_height * bounds.size.width.0 / image_width),
                )
            }
        }
        PrintImageFit::Cover => {
            if bounds_ratio > image_ratio {
                crate::size(
                    bounds.size.width,
                    Pixels(image_height * bounds.size.width.0 / image_width),
                )
            } else {
                crate::size(
                    Pixels(image_width * bounds.size.height.0 / image_height),
                    bounds.size.height,
                )
            }
        }
        PrintImageFit::ScaleDown => {
            if image_width > bounds.size.width.0 || image_height > bounds.size.height.0 {
                return fitted_image_bounds(bounds, image, PrintImageFit::Contain, frame_index);
            }
            crate::size(Pixels(image_width), Pixels(image_height))
        }
        PrintImageFit::None => crate::size(Pixels(image_width), Pixels(image_height)),
    };
    Ok(Bounds::new(
        crate::point(
            bounds.origin.x + (bounds.size.width - fitted_size.width) / 2.0,
            bounds.origin.y + (bounds.size.height - fitted_size.height) / 2.0,
        ),
        fitted_size,
    ))
}

fn offset_bounds(bounds: Bounds<Pixels>, margins: Edges<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        crate::point(
            margins.left + bounds.origin.x,
            margins.top + bounds.origin.y,
        ),
        bounds.size,
    )
}

#[derive(Default)]
struct ImageRenderBudget {
    pixels: u64,
    data_url_bytes: usize,
}

fn render_image_data_url(
    image: &RenderImage,
    frame_index: usize,
    budget: &mut ImageRenderBudget,
) -> Result<String> {
    let dimensions = image.size(frame_index);
    let width = u32::from(dimensions.width);
    let height = u32::from(dimensions.height);
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .context("print image dimensions overflowed")?;
    ensure!(
        pixels > 0 && pixels <= MAX_PRINT_IMAGE_PIXELS,
        "print images cannot exceed {MAX_PRINT_IMAGE_PIXELS} pixels"
    );
    budget.pixels = budget
        .pixels
        .checked_add(pixels)
        .context("print image pixel budget overflowed")?;
    ensure!(
        budget.pixels <= MAX_PRINT_TOTAL_IMAGE_PIXELS,
        "print jobs cannot exceed {MAX_PRINT_TOTAL_IMAGE_PIXELS} unique image pixels"
    );
    let bytes = image
        .as_bytes(frame_index)
        .context("print image frame bytes are unavailable")?;
    let expected = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .context("print image byte length overflowed")?;
    ensure!(
        bytes.len() == expected,
        "print image frame has an invalid BGRA byte length"
    );
    let mut rgba = bytes.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(width, height, rgba)
        .context("print image frame dimensions are invalid")?;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(buffer)
        .write_to(&mut encoded, ImageFormat::Png)
        .context("encoding print image frame")?;
    let png = encoded.into_inner();
    let base64_len = png
        .len()
        .checked_add(2)
        .and_then(|length| (length / 3).checked_mul(4))
        .and_then(|length| length.checked_add("data:image/png;base64,".len()))
        .context("print image data URL length overflowed")?;
    ensure!(
        base64_len <= MAX_PRINT_IMAGE_DATA_URL_BYTES,
        "encoded print images cannot exceed {MAX_PRINT_IMAGE_DATA_URL_BYTES} data-URL bytes"
    );
    budget.data_url_bytes = budget
        .data_url_bytes
        .checked_add(base64_len)
        .context("total print image data URL length overflowed")?;
    ensure!(
        budget.data_url_bytes <= MAX_PRINT_TOTAL_IMAGE_DATA_URL_BYTES,
        "print jobs cannot exceed {MAX_PRINT_TOTAL_IMAGE_DATA_URL_BYTES} unique image data-URL bytes"
    );
    let data_url = format!("data:image/png;base64,{}", BASE64.encode(png));
    ensure!(
        data_url.len() == base64_len,
        "print image data URL length was not encoded as expected"
    );
    Ok(data_url)
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
struct PdfPageImage {
    width: u32,
    height: u32,
    zlib_rgb: Vec<u8>,
}

fn page_raster_from_pixmap(pixmap: &resvg::tiny_skia::Pixmap) -> Result<PrintPageRaster> {
    let pixels = u64::from(pixmap.width())
        .checked_mul(u64::from(pixmap.height()))
        .context("print raster dimensions overflowed")?;
    let bgra_len = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .context("print BGRA buffer length overflowed")?;
    let mut bgra = Vec::new();
    bgra.try_reserve_exact(bgra_len)
        .context("allocating print BGRA buffer")?;
    for pixel in pixmap.data().chunks_exact(4) {
        // tiny-skia stores premultiplied RGBA. The page has an opaque white
        // background, but flatten defensively so malformed alpha never darkens.
        let alpha = u16::from(pixel[3]);
        let inverse = 255u16.saturating_sub(alpha);
        bgra.push((u16::from(pixel[2]) + inverse).min(255) as u8);
        bgra.push((u16::from(pixel[1]) + inverse).min(255) as u8);
        bgra.push((u16::from(pixel[0]) + inverse).min(255) as u8);
        bgra.push(255);
    }
    ensure!(
        bgra.len() == bgra_len,
        "print BGRA raster length is invalid"
    );
    Ok(PrintPageRaster {
        width: pixmap.width(),
        height: pixmap.height(),
        bgra,
    })
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
fn png_predictor_stream_from_raster(raster: &PrintPageRaster) -> Result<PdfPageImage> {
    let pixels = u64::from(raster.width)
        .checked_mul(u64::from(raster.height))
        .context("print raster dimensions overflowed")?;
    let rgb_len = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(3))
        .context("print RGB buffer length overflowed")?;
    let mut rgb = Vec::new();
    rgb.try_reserve_exact(rgb_len)
        .context("allocating print RGB buffer")?;
    for pixel in raster.bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
    }
    ensure!(rgb.len() == rgb_len, "print RGB raster length is invalid");
    let image = RgbImage::from_raw(raster.width, raster.height, rgb)
        .context("print RGB raster dimensions are invalid")?;
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut png, ImageFormat::Png)
        .context("compressing print page raster")?;
    let zlib_rgb = extract_png_idat(png.get_ref())?;
    Ok(PdfPageImage {
        width: raster.width,
        height: raster.height,
        zlib_rgb,
    })
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
fn extract_png_idat(png: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        png.starts_with(b"\x89PNG\r\n\x1a\n"),
        "encoded print raster is not a PNG"
    );
    let mut offset = 8usize;
    let mut idat = Vec::new();
    let mut saw_end = false;
    while offset < png.len() {
        let header_end = offset
            .checked_add(8)
            .context("PNG chunk header overflowed")?;
        ensure!(header_end <= png.len(), "PNG chunk header is truncated");
        let length = u32::from_be_bytes(
            png[offset..offset + 4]
                .try_into()
                .map_err(|_| anyhow!("PNG chunk length is malformed"))?,
        ) as usize;
        let kind = &png[offset + 4..offset + 8];
        let data_start = header_end;
        let data_end = data_start
            .checked_add(length)
            .context("PNG chunk length overflowed")?;
        let chunk_end = data_end
            .checked_add(4)
            .context("PNG CRC offset overflowed")?;
        ensure!(chunk_end <= png.len(), "PNG chunk is truncated");
        if kind == b"IDAT" {
            let new_len = idat
                .len()
                .checked_add(length)
                .context("PNG IDAT length overflowed")?;
            ensure!(
                new_len <= MAX_PDF_BYTES,
                "PNG IDAT exceeds the PDF byte limit"
            );
            idat.extend_from_slice(&png[data_start..data_end]);
        } else if kind == b"IEND" {
            saw_end = true;
            break;
        }
        offset = chunk_end;
    }
    ensure!(
        saw_end && !idat.is_empty(),
        "PNG has no complete IDAT stream"
    );
    Ok(idat)
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
fn write_pdf(
    title: &str,
    page_width: f64,
    page_height: f64,
    pages: &[PdfPageImage],
) -> Result<Vec<u8>> {
    let object_count = 3usize
        .checked_add(
            pages
                .len()
                .checked_mul(3)
                .context("PDF object count overflowed")?,
        )
        .context("PDF object count overflowed")?;
    let mut objects = vec![Vec::new(); object_count + 1];
    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();

    let mut kids = String::new();
    for index in 0..pages.len() {
        write!(kids, "{} 0 R ", 4 + index * 3)?;
    }
    objects[2] = format!("<< /Type /Pages /Count {} /Kids [{}] >>", pages.len(), kids).into_bytes();
    objects[3] = format!("<< /Title <{}> /Producer (Kael) >>", pdf_utf16_hex(title)).into_bytes();

    for (index, image) in pages.iter().enumerate() {
        let page_id = 4 + index * 3;
        let content_id = page_id + 1;
        let image_id = page_id + 2;
        let image_name = format!("Im{index}");
        objects[page_id] = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_width:.4} {page_height:.4}] /Resources << /XObject << /{image_name} {image_id} 0 R >> >> /Contents {content_id} 0 R >>"
        )
        .into_bytes();
        let content =
            format!("q\n{page_width:.4} 0 0 {page_height:.4} 0 0 cm\n/{image_name} Do\nQ\n");
        objects[content_id] = pdf_stream("", content.as_bytes());
        let dictionary = format!(
            "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /DecodeParms << /Predictor 15 /Colors 3 /BitsPerComponent 8 /Columns {} >>",
            image.width, image.height, image.width
        );
        objects[image_id] = pdf_stream(&dictionary, &image.zlib_rgb);
    }

    let mut pdf = Vec::with_capacity(
        pages
            .iter()
            .map(|page| page.zlib_rgb.len())
            .sum::<usize>()
            .saturating_add(64 * 1024),
    );
    pdf.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = vec![0usize; object_count + 1];
    for id in 1..=object_count {
        offsets[id] = pdf.len();
        write!(&mut ByteWriter(&mut pdf), "{id} 0 obj\n")?;
        pdf.extend_from_slice(&objects[id]);
        pdf.extend_from_slice(b"\nendobj\n");
        ensure!(
            pdf.len() <= MAX_PDF_BYTES,
            "PDF exceeded the {MAX_PDF_BYTES}-byte limit"
        );
    }
    let xref_offset = pdf.len();
    write!(
        &mut ByteWriter(&mut pdf),
        "xref\n0 {}\n0000000000 65535 f \n",
        object_count + 1
    )?;
    for offset in offsets.iter().skip(1) {
        ensure!(
            *offset <= 9_999_999_999usize,
            "PDF xref offset is too large"
        );
        write!(&mut ByteWriter(&mut pdf), "{offset:010} 00000 n \n")?;
    }
    write!(
        &mut ByteWriter(&mut pdf),
        "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        object_count + 1
    )?;
    ensure!(
        pdf.len() <= MAX_PDF_BYTES,
        "PDF exceeded the {MAX_PDF_BYTES}-byte limit"
    );
    Ok(pdf)
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
struct ByteWriter<'a>(&'a mut Vec<u8>);

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
impl std::fmt::Write for ByteWriter<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.extend_from_slice(value.as_bytes());
        Ok(())
    }
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
fn pdf_stream(dictionary: &str, bytes: &[u8]) -> Vec<u8> {
    let mut stream = Vec::with_capacity(bytes.len().saturating_add(dictionary.len() + 64));
    stream.extend_from_slice(
        format!("<< {dictionary} /Length {} >>\nstream\n", bytes.len()).as_bytes(),
    );
    stream.extend_from_slice(bytes);
    stream.extend_from_slice(b"\nendstream");
    stream
}

#[cfg(any(test, target_os = "linux", target_os = "freebsd"))]
fn pdf_utf16_hex(value: &str) -> String {
    let mut result = String::with_capacity(value.len().saturating_mul(4).saturating_add(4));
    result.push_str("FEFF");
    for unit in value.encode_utf16() {
        let _ = write!(result, "{unit:04X}");
    }
    result
}

fn font_family_attribute(style: &PrintTextStyle) -> String {
    style.font_family_ref().map_or_else(String::new, |family| {
        format!(" font-family=\"{}\"", escape_xml(family.as_ref()))
    })
}

fn color_hex(color: Rgba) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn alpha(color: Rgba) -> f32 {
    color.a.clamp(0.0, 1.0)
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PlatformPrintPage, PrintImageStyle, PrintOrientation, PrintStroke, PrintTextStyle, point,
        px, rgb, size,
    };
    use image::{Frame, ImageBuffer, Rgba as ImageRgba};

    fn complete_job() -> PlatformPrintJob {
        let image = Arc::new(RenderImage::new(vec![Frame::new(
            ImageBuffer::<ImageRgba<u8>, _>::from_raw(
                2,
                1,
                // RenderImage bytes are BGRA.
                vec![0, 0, 255, 255, 255, 0, 0, 128],
            )
            .unwrap(),
        )]));
        PlatformPrintJob {
            title: "Kael 🗎".into(),
            orientation: PrintOrientation::Portrait,
            margins: Edges::all(px(12.0)),
            page_size: size(px(144.0), px(180.0)),
            pages: vec![PlatformPrintPage {
                commands: vec![
                    PrintCommand::FillRect {
                        bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(120.0), px(156.0))),
                        color: rgb(0xffffff),
                    },
                    PrintCommand::FillRoundedRect {
                        bounds: Bounds::new(point(px(4.0), px(4.0)), size(px(50.0), px(20.0))),
                        radius: px(4.0),
                        color: crate::rgba(0x336699cc),
                    },
                    PrintCommand::StrokeRect {
                        bounds: Bounds::new(point(px(4.0), px(28.0)), size(px(50.0), px(20.0))),
                        stroke: PrintStroke::new(px(1.5)).color(rgb(0x123456)),
                    },
                    PrintCommand::StrokeRoundedRect {
                        bounds: Bounds::new(point(px(58.0), px(4.0)), size(px(58.0), px(20.0))),
                        radius: px(5.0),
                        stroke: PrintStroke::new(px(2.0)).color(rgb(0x654321)),
                    },
                    PrintCommand::StrokeLine {
                        from: point(px(4.0), px(52.0)),
                        to: point(px(116.0), px(52.0)),
                        stroke: PrintStroke::new(px(1.0)),
                    },
                    PrintCommand::Text {
                        origin: point(px(4.0), px(58.0)),
                        text: "Unicode: 漢字".into(),
                        style: PrintTextStyle::new(px(11.0)).color(rgb(0x112233)),
                    },
                    PrintCommand::TextBlock {
                        bounds: Bounds::new(point(px(4.0), px(74.0)), size(px(70.0), px(42.0))),
                        text: "A wrapped block with a longwordthatmustsplit safely.".into(),
                        style: PrintTextStyle::new(px(9.0)),
                    },
                    PrintCommand::Image {
                        bounds: Bounds::new(point(px(78.0), px(74.0)), size(px(38.0), px(42.0))),
                        image,
                        style: PrintImageStyle::new()
                            .object_fit(PrintImageFit::Cover)
                            .opacity(0.5),
                    },
                ],
            }],
        }
    }

    #[test]
    fn renders_every_print_command_into_a_structurally_complete_pdf() {
        let pdf = render_print_job_pdf(&complete_job()).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.7"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 1"));
        assert!(text.contains("/Subtype /Image"));
        assert!(text.contains("/Predictor 15"));
        let startxref = text
            .rsplit_once("startxref\n")
            .and_then(|(_, tail)| tail.lines().next())
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(&pdf[startxref..startxref + 4], b"xref");
    }

    #[test]
    fn serializes_every_print_command_with_image_clip_and_opacity() {
        let job = complete_job();
        let (page_width, page_height) = oriented_page_size(&job);
        let mut image_cache = HashMap::new();
        let mut image_budget = ImageRenderBudget::default();
        let svg = render_page_svg(
            0,
            &job.pages[0].commands,
            page_width,
            page_height,
            job.margins,
            &mut image_cache,
            &mut image_budget,
        )
        .unwrap();

        assert!(
            svg.contains("<rect x=\"12\" y=\"12\" width=\"120\" height=\"156\" fill=\"#ffffff\"")
        );
        assert!(svg.contains(
            "<rect x=\"16\" y=\"16\" width=\"50\" height=\"20\" rx=\"4\" ry=\"4\" fill=\"#336699\""
        ));
        assert!(svg.contains("stroke=\"#123456\" stroke-opacity=\"1\" stroke-width=\"1.5\""));
        assert!(svg.contains("rx=\"5\" ry=\"5\" fill=\"none\" stroke=\"#654321\""));
        assert!(svg.contains("<line x1=\"16\" y1=\"64\" x2=\"128\" y2=\"64\""));
        assert!(svg.contains("font-size=\"11\" fill=\"#112233\""));
        assert!(svg.contains("Unicode: 漢字"));
        assert!(svg.contains("clipPath id=\"p0c6t\""));
        assert!(svg.contains("<tspan"));
        assert!(svg.contains("clipPath id=\"p0c7i\""));
        assert!(svg.contains("<image "));
        assert!(svg.contains("opacity=\"0.5\""));
        assert!(svg.contains("clip-path=\"url(#p0c7i)\""));
        assert_eq!(svg.matches("href=\"data:image/png;base64,").count(), 1);
        assert!(!svg.contains("xlink:href"));
    }

    #[test]
    fn rejects_empty_and_pathologically_large_jobs_before_allocating_pages() {
        let mut job = complete_job();
        job.pages.clear();
        assert!(render_print_job_pdf(&job).is_err());

        let mut job = complete_job();
        job.page_size = size(px(100_000.0), px(100_000.0));
        assert!(render_print_job_pdf(&job).is_err());
    }

    #[test]
    fn rejects_adversarial_command_and_text_budgets_before_rasterization() {
        let mut command_heavy = complete_job();
        command_heavy.pages[0].commands = vec![
            PrintCommand::FillRect {
                bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(1.0), px(1.0)),),
                color: rgb(0),
            };
            MAX_PRINT_COMMANDS + 1
        ];
        assert!(render_print_job_pages(&command_heavy).is_err());

        let mut text_heavy = complete_job();
        text_heavy.pages[0].commands = vec![PrintCommand::Text {
            origin: point(px(0.0), px(0.0)),
            text: "&".repeat(MAX_PRINT_TEXT_BYTES + 1).into(),
            style: PrintTextStyle::default(),
        }];
        assert!(render_print_job_pages(&text_heavy).is_err());
    }

    #[test]
    fn rejects_page_and_unique_image_budgets() {
        let mut page_heavy = complete_job();
        page_heavy.pages = (0..=MAX_PRINT_PAGES)
            .map(|_| PlatformPrintPage {
                commands: Vec::new(),
            })
            .collect();
        assert!(render_print_job_pages(&page_heavy).is_err());

        let job = complete_job();
        let PrintCommand::Image { image, style, .. } = &job.pages[0].commands[7] else {
            panic!("complete print job lost its image command");
        };
        let mut pixel_budget = ImageRenderBudget {
            pixels: MAX_PRINT_TOTAL_IMAGE_PIXELS,
            data_url_bytes: 0,
        };
        assert!(
            render_image_data_url(image, style.selected_frame_index(), &mut pixel_budget).is_err()
        );

        let mut encoded_budget = ImageRenderBudget {
            pixels: 0,
            data_url_bytes: MAX_PRINT_TOTAL_IMAGE_DATA_URL_BYTES,
        };
        assert!(
            render_image_data_url(image, style.selected_frame_index(), &mut encoded_budget)
                .is_err()
        );
    }

    #[test]
    fn wrap_text_bounds_unbroken_unicode_without_losing_valid_utf8() {
        let lines = wrap_text("漢字漢字漢", 2, 3);
        assert_eq!(lines, vec!["漢字", "漢字", "漢"]);
    }

    #[test]
    fn png_chunk_parser_rejects_truncation() {
        assert!(extract_png_idat(b"\x89PNG\r\n\x1a\n\0").is_err());
    }
}
