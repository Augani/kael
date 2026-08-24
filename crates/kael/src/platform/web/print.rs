use crate::{
    Bounds, Edges, Pixels, PlatformPrintJob, PlatformPrintPage, PrintCommand, PrintImageFit,
    PrintOrientation, PrintStroke, PrintTextStyle, RenderImage, Rgba, point, px, size,
};
use anyhow::{Context as _, Result, anyhow, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, ImageFormat, RgbaImage};
use std::{collections::HashMap, fmt::Write as _, io::Cursor};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use web_sys::{Event, EventTarget, HtmlCanvasElement, HtmlIFrameElement};

const MAX_PRINT_PAGES: usize = 256;
const MAX_PRINT_COMMANDS: usize = 50_000;
const MAX_PRINT_IMAGE_PIXELS: u64 = 16_777_216;
const MAX_PRINT_TOTAL_IMAGE_PIXELS: u64 = 33_554_432;
const MAX_PRINT_HTML_BYTES: usize = 134_217_728;

struct BrowserPrintFrame {
    iframe: HtmlIFrameElement,
    load_callback: Closure<dyn FnMut(Event)>,
    afterprint_target: EventTarget,
    afterprint_callback: Closure<dyn FnMut(Event)>,
}

impl Drop for BrowserPrintFrame {
    fn drop(&mut self) {
        let _ = self.iframe.remove_event_listener_with_callback(
            "load",
            self.load_callback.as_ref().unchecked_ref(),
        );
        let _ = self.afterprint_target.remove_event_listener_with_callback(
            "afterprint",
            self.afterprint_callback.as_ref().unchecked_ref(),
        );
        self.iframe.remove();
    }
}

/// Owns the isolated printable iframe used for Kael-rendered print jobs.
pub(super) struct BrowserPrintManager {
    canvas: HtmlCanvasElement,
    active_frame: Option<BrowserPrintFrame>,
}

impl BrowserPrintManager {
    pub(super) fn new(canvas: &HtmlCanvasElement) -> Result<Self> {
        validate_print_backend()?;
        canvas
            .set_attribute("data-kael-print-capability", "print-job-dialog")
            .map_err(js_error)?;
        Ok(Self {
            canvas: canvas.clone(),
            active_frame: None,
        })
    }

    pub(super) fn print(&mut self, job: PlatformPrintJob) -> Result<()> {
        let page_count = job.pages.len();
        let html = print_job_html(&job)?;
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser printing requires a Document")?;
        let body = document
            .body()
            .context("browser printing requires a document body")?;
        let iframe = document
            .create_element("iframe")
            .map_err(js_error)?
            .dyn_into::<HtmlIFrameElement>()
            .map_err(|_| anyhow!("browser print frame was not an iframe"))?;
        iframe
            .set_attribute("data-kael-print-frame", "true")
            .map_err(js_error)?;
        iframe
            .set_attribute("title", "Kael print output")
            .map_err(js_error)?;
        iframe
            .set_attribute("aria-hidden", "true")
            .map_err(js_error)?;
        let style = iframe.style();
        for (name, value) in [
            ("position", "fixed"),
            ("right", "0"),
            ("bottom", "0"),
            ("width", "1px"),
            ("height", "1px"),
            ("border", "0"),
            ("opacity", "0"),
            ("pointer-events", "none"),
        ] {
            style.set_property(name, value).map_err(js_error)?;
        }
        iframe.set_srcdoc(&html);

        let iframe_for_load = iframe.clone();
        let canvas_for_load = self.canvas.clone();
        let load_callback = Closure::wrap(Box::new(move |_event: Event| {
            let result = iframe_for_load
                .content_window()
                .context("browser print frame Window is unavailable")
                .and_then(|window| window.print().map_err(js_error));
            let status = if result.is_ok() {
                "dialog-returned"
            } else {
                "dialog-failed"
            };
            let _ = canvas_for_load.set_attribute("data-kael-print-status", status);
            if let Err(error) = result {
                log::error!("browser print dialog failed: {error:#}");
            }
        }) as Box<dyn FnMut(Event)>);
        iframe
            .add_event_listener_with_callback("load", load_callback.as_ref().unchecked_ref())
            .map_err(js_error)?;
        body.append_child(&iframe).map_err(js_error)?;

        let frame_window = iframe
            .content_window()
            .context("browser print frame Window is unavailable")?;
        let afterprint_target: EventTarget = frame_window.unchecked_into();
        let canvas_for_afterprint = self.canvas.clone();
        let iframe_for_afterprint = iframe.clone();
        let load_listener_for_afterprint: js_sys::Function = load_callback
            .as_ref()
            .unchecked_ref::<js_sys::Function>()
            .clone();
        let afterprint_callback = Closure::wrap(Box::new(move |_event: Event| {
            let _ = canvas_for_afterprint.set_attribute("data-kael-print-status", "dialog-closed");
            // A printable snapshot can contain large inline PNGs. Replace its DOM and srcdoc
            // before detaching the frame so a completed print does not retain that payload.
            // Remove the load handler first: blanking srcdoc must not open a second dialog.
            let _ = iframe_for_afterprint
                .remove_event_listener_with_callback("load", &load_listener_for_afterprint);
            if let Some(document) = iframe_for_afterprint.content_document()
                && let Some(root) = document.document_element()
            {
                root.set_inner_html("<head></head><body></body>");
            }
            iframe_for_afterprint.set_srcdoc("");
            iframe_for_afterprint.remove();
        }) as Box<dyn FnMut(Event)>);
        afterprint_target
            .add_event_listener_with_callback(
                "afterprint",
                afterprint_callback.as_ref().unchecked_ref(),
            )
            .map_err(js_error)?;

        self.canvas
            .set_attribute("data-kael-print-status", "snapshot-ready")
            .map_err(js_error)?;
        self.canvas
            .set_attribute("data-kael-print-pages", &page_count.to_string())
            .map_err(js_error)?;
        self.active_frame = Some(BrowserPrintFrame {
            iframe,
            load_callback,
            afterprint_target,
            afterprint_callback,
        });
        Ok(())
    }
}

fn validate_print_backend() -> Result<()> {
    let probe = PlatformPrintJob {
        title: "Kael browser print probe".into(),
        orientation: PrintOrientation::Portrait,
        margins: Edges {
            top: px(18.0),
            right: px(18.0),
            bottom: px(18.0),
            left: px(18.0),
        },
        page_size: size(px(144.0), px(144.0)),
        pages: vec![PlatformPrintPage {
            commands: vec![
                PrintCommand::FillRect {
                    bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(108.0), px(108.0))),
                    color: crate::rgb(0xffffff),
                },
                PrintCommand::Text {
                    origin: point(px(8.0), px(8.0)),
                    text: "Kael".into(),
                    style: PrintTextStyle::default(),
                },
            ],
        }],
    };
    let html = print_job_html(&probe)?;
    ensure!(
        html.contains("class=\"kael-print-page\"")
            && html.contains("Kael")
            && html.contains("background:rgba(255,255,255,1)"),
        "browser print backend self-check did not retain page commands"
    );
    Ok(())
}

fn print_job_html(job: &PlatformPrintJob) -> Result<String> {
    ensure!(
        !job.pages.is_empty(),
        "browser print jobs must contain at least one page"
    );
    ensure!(
        job.pages.len() <= MAX_PRINT_PAGES,
        "browser print jobs cannot exceed {MAX_PRINT_PAGES} pages"
    );
    let command_count = job
        .pages
        .iter()
        .try_fold(0usize, |count, page| count.checked_add(page.commands.len()))
        .context("browser print command count overflowed")?;
    ensure!(
        command_count <= MAX_PRINT_COMMANDS,
        "browser print jobs cannot exceed {MAX_PRINT_COMMANDS} drawing commands"
    );

    let (page_width, page_height) = match job.orientation {
        PrintOrientation::Portrait => (
            f64::from(job.page_size.width.0),
            f64::from(job.page_size.height.0),
        ),
        PrintOrientation::Landscape => (
            f64::from(job.page_size.height.0),
            f64::from(job.page_size.width.0),
        ),
    };
    let mut html = String::with_capacity(command_count.saturating_mul(128).min(8_388_608));
    write!(
        html,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>\
         @page{{size:{page_width}pt {page_height}pt;margin:0}}\
         *{{box-sizing:border-box}}html,body{{margin:0;padding:0;background:white}}\
         .kael-print-page{{position:relative;width:{page_width}pt;height:{page_height}pt;overflow:hidden;background:white;break-after:page;page-break-after:always}}\
         .kael-print-page:last-child{{break-after:auto;page-break-after:auto}}\
         @media screen{{body{{background:#ddd}}.kael-print-page{{margin:12px auto;box-shadow:0 1px 8px #0004}}}}\
         </style></head><body data-kael-print-pages=\"{}\" data-kael-print-orientation=\"{}\">",
        escape_html(job.title.as_ref()),
        job.pages.len(),
        job.orientation.to_text(),
    )?;

    let mut image_cache = HashMap::new();
    let mut total_image_pixels = 0u64;
    for (page_index, page) in job.pages.iter().enumerate() {
        write!(
            html,
            "<section class=\"kael-print-page\" role=\"document\" aria-label=\"Page {}\">",
            page_index + 1
        )?;
        for command in &page.commands {
            write_print_command(
                &mut html,
                command,
                job.margins,
                &mut image_cache,
                &mut total_image_pixels,
            )?;
            ensure!(
                html.len() <= MAX_PRINT_HTML_BYTES,
                "browser printable snapshot exceeded {MAX_PRINT_HTML_BYTES} bytes"
            );
        }
        html.push_str("</section>");
    }
    html.push_str("</body></html>");
    Ok(html)
}

fn write_print_command(
    html: &mut String,
    command: &PrintCommand,
    margins: Edges<Pixels>,
    image_cache: &mut HashMap<(crate::ImageId, usize), String>,
    total_image_pixels: &mut u64,
) -> Result<()> {
    match command {
        PrintCommand::FillRect { bounds, color } => write!(
            html,
            "<div aria-hidden=\"true\" style=\"{}background:{}\"></div>",
            bounds_css(*bounds, margins),
            color_css(*color)
        )?,
        PrintCommand::FillRoundedRect {
            bounds,
            radius,
            color,
        } => write!(
            html,
            "<div aria-hidden=\"true\" style=\"{}border-radius:{}pt;background:{}\"></div>",
            bounds_css(*bounds, margins),
            radius.0,
            color_css(*color)
        )?,
        PrintCommand::StrokeRect { bounds, stroke } => {
            write_stroked_rect(html, *bounds, margins, None, stroke)?
        }
        PrintCommand::StrokeRoundedRect {
            bounds,
            radius,
            stroke,
        } => write_stroked_rect(html, *bounds, margins, Some(*radius), stroke)?,
        PrintCommand::StrokeLine { from, to, stroke } => {
            let x = f64::from((margins.left + from.x).0);
            let y = f64::from((margins.top + from.y).0);
            let dx = f64::from((to.x - from.x).0);
            let dy = f64::from((to.y - from.y).0);
            let length = dx.hypot(dy);
            let angle = dy.atan2(dx).to_degrees();
            write!(
                html,
                "<div aria-hidden=\"true\" style=\"position:absolute;left:{x}pt;top:{y}pt;width:{length}pt;height:{}pt;background:{};transform-origin:0 50%;transform:translateY(-50%) rotate({angle}deg)\"></div>",
                stroke.width().0,
                color_css(stroke.color_ref())
            )?;
        }
        PrintCommand::Text {
            origin,
            text,
            style,
        } => write!(
            html,
            "<div style=\"position:absolute;left:{}pt;top:{}pt;white-space:pre;{}\">{}</div>",
            (margins.left + origin.x).0,
            (margins.top + origin.y).0,
            text_style_css(style),
            escape_html(text.as_ref())
        )?,
        PrintCommand::TextBlock {
            bounds,
            text,
            style,
        } => write!(
            html,
            "<div style=\"{}overflow:hidden;white-space:pre-wrap;overflow-wrap:anywhere;{}\">{}</div>",
            bounds_css(*bounds, margins),
            text_style_css(style),
            escape_html(text.as_ref())
        )?,
        PrintCommand::Image {
            bounds,
            image,
            style,
        } => {
            let frame_index = style.selected_frame_index();
            let key = (image.id, frame_index);
            let source = if let Some(source) = image_cache.get(&key) {
                source.clone()
            } else {
                let source = render_image_data_url(image, frame_index, total_image_pixels)?;
                image_cache.insert(key, source.clone());
                source
            };
            write!(
                html,
                "<div aria-hidden=\"true\" style=\"{}overflow:hidden;opacity:{}\"><img alt=\"\" src=\"{}\" style=\"display:block;width:100%;height:100%;object-fit:{}\"></div>",
                bounds_css(*bounds, margins),
                style.opacity_ref(),
                source,
                object_fit_css(style.object_fit_ref())
            )?;
        }
    }
    Ok(())
}

fn write_stroked_rect(
    html: &mut String,
    bounds: Bounds<Pixels>,
    margins: Edges<Pixels>,
    radius: Option<Pixels>,
    stroke: &PrintStroke,
) -> Result<()> {
    write!(
        html,
        "<div aria-hidden=\"true\" style=\"{}border:{}pt solid {};{}\"></div>",
        bounds_css(bounds, margins),
        stroke.width().0,
        color_css(stroke.color_ref()),
        radius.map_or_else(String::new, |radius| format!(
            "border-radius:{}pt",
            radius.0
        ))
    )?;
    Ok(())
}

fn bounds_css(bounds: Bounds<Pixels>, margins: Edges<Pixels>) -> String {
    format!(
        "position:absolute;left:{}pt;top:{}pt;width:{}pt;height:{}pt;",
        (margins.left + bounds.origin.x).0,
        (margins.top + bounds.origin.y).0,
        bounds.size.width.0,
        bounds.size.height.0
    )
}

fn text_style_css(style: &PrintTextStyle) -> String {
    let family = style.font_family_ref().map_or_else(
        || "system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif".to_owned(),
        |family| format!("\"{}\"", escape_css_string(family.as_ref())),
    );
    format!(
        "font-family:{family};font-size:{}pt;line-height:1.2;color:{};",
        style.font_size().0,
        color_css(style.color_ref())
    )
}

fn color_css(color: Rgba) -> String {
    format!(
        "rgba({},{},{},{})",
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        color.a.clamp(0.0, 1.0)
    )
}

fn object_fit_css(fit: PrintImageFit) -> &'static str {
    match fit {
        PrintImageFit::Fill => "fill",
        PrintImageFit::Contain => "contain",
        PrintImageFit::Cover => "cover",
        PrintImageFit::ScaleDown => "scale-down",
        PrintImageFit::None => "none",
    }
}

fn render_image_data_url(
    image: &RenderImage,
    frame_index: usize,
    total_image_pixels: &mut u64,
) -> Result<String> {
    let size = image.size(frame_index);
    let width = u32::from(size.width);
    let height = u32::from(size.height);
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .context("browser print image dimensions overflowed")?;
    ensure!(
        pixels <= MAX_PRINT_IMAGE_PIXELS,
        "browser print images cannot exceed {MAX_PRINT_IMAGE_PIXELS} pixels"
    );
    *total_image_pixels = total_image_pixels
        .checked_add(pixels)
        .context("browser print image budget overflowed")?;
    ensure!(
        *total_image_pixels <= MAX_PRINT_TOTAL_IMAGE_PIXELS,
        "browser print jobs cannot exceed {MAX_PRINT_TOTAL_IMAGE_PIXELS} unique image pixels"
    );

    let bytes = image
        .as_bytes(frame_index)
        .context("browser print image frame bytes are unavailable")?;
    let expected_len = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .context("browser print image byte length overflowed")?;
    ensure!(
        bytes.len() == expected_len,
        "browser print image frame has an invalid BGRA byte length"
    );
    let mut rgba = bytes.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(width, height, rgba)
        .context("browser print image frame dimensions are invalid")?;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(buffer).write_to(&mut encoded, ImageFormat::Png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64.encode(encoded.into_inner())
    ))
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn escape_css_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\a "),
            '\r' => escaped.push_str("\\d "),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_css_escaping_do_not_allow_markup_injection() {
        assert_eq!(
            escape_html("<script title=\"x\">&'"),
            "&lt;script title=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(escape_css_string("A\\\"B"), "A\\\\\\\"B");
    }

    #[test]
    fn print_fit_maps_to_browser_object_fit() {
        assert_eq!(object_fit_css(PrintImageFit::Fill), "fill");
        assert_eq!(object_fit_css(PrintImageFit::Contain), "contain");
        assert_eq!(object_fit_css(PrintImageFit::Cover), "cover");
        assert_eq!(object_fit_css(PrintImageFit::ScaleDown), "scale-down");
        assert_eq!(object_fit_css(PrintImageFit::None), "none");
    }
}
