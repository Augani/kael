use crate::{
    App, Bounds, Edges, ObjectFit, Pixels, Point, RenderImage, Rgba, SharedString, Size, point, px,
    rgb,
};
use anyhow::{Result, anyhow};
use std::sync::Arc;

/// A print job that can be sent directly to the platform printer or shown in a native print dialog.
pub struct PrintJob {
    title: SharedString,
    pages: Vec<PrintPage>,
    orientation: PrintOrientation,
    margins: Edges<Pixels>,
}

impl PrintJob {
    /// Creates a new print job with a title.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            pages: Vec::new(),
            orientation: PrintOrientation::Portrait,
            margins: Edges::all(px(36.)),
        }
    }

    /// Appends a page to the print job.
    pub fn page(mut self, page: PrintPage) -> Self {
        self.pages.push(page);
        self
    }

    /// Appends multiple pages to the print job.
    pub fn pages(mut self, pages: impl IntoIterator<Item = PrintPage>) -> Self {
        self.pages.extend(pages);
        self
    }

    /// Sets the orientation metadata for the print job.
    pub fn orientation(mut self, orientation: PrintOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Sets uniform or per-edge margins for every page in the print job.
    pub fn margins(mut self, margins: impl Into<Edges<Pixels>>) -> Self {
        self.margins = margins.into();
        self
    }

    pub(crate) fn into_platform_job(self, cx: &mut App) -> Result<PlatformPrintJob> {
        if self.pages.is_empty() {
            return Err(anyhow!("print jobs must contain at least one page"));
        }

        let first_page_size = self.pages[0].size;
        let mut rendered_pages = Vec::with_capacity(self.pages.len());

        for page in self.pages {
            if page.size != first_page_size {
                return Err(anyhow!(
                    "all pages in a print job must use the same page size"
                ));
            }

            let content_size = content_size_for_page(page.size, self.margins)?;
            let mut context = PrintContext::new(page.size, content_size);
            (page.render)(&mut context, cx);
            rendered_pages.push(PlatformPrintPage {
                commands: context.finish(),
            });
        }

        Ok(PlatformPrintJob {
            title: self.title,
            orientation: self.orientation,
            margins: self.margins,
            page_size: first_page_size,
            pages: rendered_pages,
        })
    }
}

/// A single page in a print job.
pub struct PrintPage {
    size: Size<Pixels>,
    render: Box<dyn Fn(&mut PrintContext, &mut App)>,
}

impl PrintPage {
    /// Creates a page with a paper size and a callback that records print commands for that page.
    pub fn new(size: Size<Pixels>, render: impl Fn(&mut PrintContext, &mut App) + 'static) -> Self {
        Self {
            size,
            render: Box::new(render),
        }
    }
}

/// Orientation metadata for a print job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintOrientation {
    /// Print the page in portrait orientation.
    Portrait,
    /// Print the page in landscape orientation.
    Landscape,
}

/// Stroke settings for line drawing commands.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintStroke {
    width: Pixels,
    color: Rgba,
}

impl PrintStroke {
    /// Creates a stroke with a width and a default black color.
    pub fn new(width: Pixels) -> Self {
        Self {
            width,
            color: rgb(0x000000),
        }
    }

    /// Sets the stroke color.
    pub fn color(mut self, color: impl Into<Rgba>) -> Self {
        self.color = color.into();
        self
    }

    pub(crate) fn width(&self) -> Pixels {
        self.width
    }

    pub(crate) fn color_ref(&self) -> Rgba {
        self.color
    }
}

/// Text styling for print text commands.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintTextStyle {
    font_family: Option<SharedString>,
    font_size: Pixels,
    color: Rgba,
}

impl PrintTextStyle {
    /// Creates a text style with the given font size, using the platform system font and black text by default.
    pub fn new(font_size: Pixels) -> Self {
        Self {
            font_family: None,
            font_size,
            color: rgb(0x000000),
        }
    }

    /// Sets the font family for the text.
    pub fn font_family(mut self, font_family: impl Into<SharedString>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// Sets the text color.
    pub fn color(mut self, color: impl Into<Rgba>) -> Self {
        self.color = color.into();
        self
    }

    pub(crate) fn font_family_ref(&self) -> Option<&SharedString> {
        self.font_family.as_ref()
    }

    pub(crate) fn font_size(&self) -> Pixels {
        self.font_size
    }

    pub(crate) fn color_ref(&self) -> Rgba {
        self.color
    }
}

impl Default for PrintTextStyle {
    fn default() -> Self {
        Self::new(px(12.))
    }
}

/// How to fit an image into a print image bounds rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintImageFit {
    /// Stretch the image to fill the full bounds.
    Fill,
    /// Preserve aspect ratio while fitting inside the bounds.
    Contain,
    /// Preserve aspect ratio while covering the full bounds.
    Cover,
    /// Only scale the image down if it would otherwise overflow.
    ScaleDown,
    /// Draw the image at its natural size.
    None,
}

impl From<ObjectFit> for PrintImageFit {
    fn from(value: ObjectFit) -> Self {
        match value {
            ObjectFit::Fill => Self::Fill,
            ObjectFit::Contain => Self::Contain,
            ObjectFit::Cover => Self::Cover,
            ObjectFit::ScaleDown => Self::ScaleDown,
            ObjectFit::None => Self::None,
        }
    }
}

/// Image layout settings for print image commands.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintImageStyle {
    object_fit: PrintImageFit,
    frame_index: usize,
}

impl PrintImageStyle {
    /// Creates an image style with `Contain` fitting and the first frame selected.
    pub fn new() -> Self {
        Self {
            object_fit: PrintImageFit::Contain,
            frame_index: 0,
        }
    }

    /// Sets how the image should fit into the target bounds.
    pub fn object_fit(mut self, object_fit: impl Into<PrintImageFit>) -> Self {
        self.object_fit = object_fit.into();
        self
    }

    /// Selects the image frame to print.
    pub fn frame_index(mut self, frame_index: usize) -> Self {
        self.frame_index = frame_index;
        self
    }

    pub(crate) fn object_fit_ref(&self) -> PrintImageFit {
        self.object_fit
    }

    pub(crate) fn selected_frame_index(&self) -> usize {
        self.frame_index
    }
}

impl Default for PrintImageStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// A recording context for one printed page.
pub struct PrintContext {
    page_size: Size<Pixels>,
    content_size: Size<Pixels>,
    commands: Vec<PrintCommand>,
}

impl PrintContext {
    fn new(page_size: Size<Pixels>, content_size: Size<Pixels>) -> Self {
        Self {
            page_size,
            content_size,
            commands: Vec::new(),
        }
    }

    /// Returns the full paper size for the current page.
    pub fn page_size(&self) -> Size<Pixels> {
        self.page_size
    }

    /// Returns the drawable content size after margins are applied.
    pub fn size(&self) -> Size<Pixels> {
        self.content_size
    }

    /// Returns the drawable content bounds after margins are applied.
    pub fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(0.)), self.content_size)
    }

    /// Fills a rectangle with a solid color.
    pub fn fill_rect(&mut self, bounds: Bounds<Pixels>, color: impl Into<Rgba>) {
        self.commands.push(PrintCommand::FillRect {
            bounds,
            color: color.into(),
        });
    }

    /// Fills a rounded rectangle with a solid color.
    pub fn fill_rounded_rect(
        &mut self,
        bounds: Bounds<Pixels>,
        radius: impl Into<Pixels>,
        color: impl Into<Rgba>,
    ) {
        self.commands.push(PrintCommand::FillRoundedRect {
            bounds,
            radius: radius.into(),
            color: color.into(),
        });
    }

    /// Strokes a rectangle outline.
    pub fn stroke_rect(&mut self, bounds: Bounds<Pixels>, stroke: PrintStroke) {
        self.commands
            .push(PrintCommand::StrokeRect { bounds, stroke });
    }

    /// Strokes a rounded rectangle outline.
    pub fn stroke_rounded_rect(
        &mut self,
        bounds: Bounds<Pixels>,
        radius: impl Into<Pixels>,
        stroke: PrintStroke,
    ) {
        self.commands.push(PrintCommand::StrokeRoundedRect {
            bounds,
            radius: radius.into(),
            stroke,
        });
    }

    /// Draws a single stroked line.
    pub fn stroke_line(&mut self, from: Point<Pixels>, to: Point<Pixels>, stroke: PrintStroke) {
        self.commands
            .push(PrintCommand::StrokeLine { from, to, stroke });
    }

    /// Draws a single-line text run at the provided origin.
    pub fn draw_text(
        &mut self,
        text: impl Into<SharedString>,
        origin: Point<Pixels>,
        style: PrintTextStyle,
    ) {
        self.commands.push(PrintCommand::Text {
            origin,
            text: text.into(),
            style,
        });
    }

    /// Draws wrapped text inside the provided bounds.
    pub fn draw_text_block(
        &mut self,
        text: impl Into<SharedString>,
        bounds: Bounds<Pixels>,
        style: PrintTextStyle,
    ) {
        self.commands.push(PrintCommand::TextBlock {
            bounds,
            text: text.into(),
            style,
        });
    }

    /// Draws an image into the provided bounds using the default image style.
    pub fn draw_image(&mut self, image: Arc<RenderImage>, bounds: Bounds<Pixels>) {
        self.draw_image_with_style(image, bounds, PrintImageStyle::default());
    }

    /// Draws an image into the provided bounds using the supplied image style.
    pub fn draw_image_with_style(
        &mut self,
        image: Arc<RenderImage>,
        bounds: Bounds<Pixels>,
        style: PrintImageStyle,
    ) {
        if image.frame_count() == 0 {
            return;
        }

        let clamped_frame_index = style.selected_frame_index().min(image.frame_count() - 1);
        let style = style.frame_index(clamped_frame_index);
        self.commands.push(PrintCommand::Image {
            bounds,
            image,
            style,
        });
    }

    fn finish(self) -> Vec<PrintCommand> {
        self.commands
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PrintCommand {
    FillRect {
        bounds: Bounds<Pixels>,
        color: Rgba,
    },
    FillRoundedRect {
        bounds: Bounds<Pixels>,
        radius: Pixels,
        color: Rgba,
    },
    StrokeRect {
        bounds: Bounds<Pixels>,
        stroke: PrintStroke,
    },
    StrokeRoundedRect {
        bounds: Bounds<Pixels>,
        radius: Pixels,
        stroke: PrintStroke,
    },
    StrokeLine {
        from: Point<Pixels>,
        to: Point<Pixels>,
        stroke: PrintStroke,
    },
    Text {
        origin: Point<Pixels>,
        text: SharedString,
        style: PrintTextStyle,
    },
    TextBlock {
        bounds: Bounds<Pixels>,
        text: SharedString,
        style: PrintTextStyle,
    },
    Image {
        bounds: Bounds<Pixels>,
        image: Arc<RenderImage>,
        style: PrintImageStyle,
    },
}

pub(crate) struct PlatformPrintJob {
    pub(crate) title: SharedString,
    pub(crate) orientation: PrintOrientation,
    pub(crate) margins: Edges<Pixels>,
    pub(crate) page_size: Size<Pixels>,
    pub(crate) pages: Vec<PlatformPrintPage>,
}

pub(crate) struct PlatformPrintPage {
    pub(crate) commands: Vec<PrintCommand>,
}

fn content_size_for_page(page_size: Size<Pixels>, margins: Edges<Pixels>) -> Result<Size<Pixels>> {
    let content_width = page_size.width.0 - margins.left.0 - margins.right.0;
    let content_height = page_size.height.0 - margins.top.0 - margins.bottom.0;

    if content_width <= 0.0 || content_height <= 0.0 {
        return Err(anyhow!(
            "page margins leave no drawable space for print content"
        ));
    }

    Ok(Size::new(px(content_width), px(content_height)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, size};
    use image::{Frame, RgbaImage};
    use smallvec::SmallVec;

    #[test]
    fn print_context_records_commands() {
        let mut context = PrintContext::new(size(px(612.), px(792.)), size(px(540.), px(720.)));
        context.fill_rect(
            Bounds::new(point(px(0.), px(0.)), size(px(100.), px(50.))),
            rgb(0xff0000),
        );
        context.fill_rounded_rect(
            Bounds::new(point(px(0.), px(60.)), size(px(100.), px(50.))),
            px(8.),
            rgb(0x00ff00),
        );
        context.stroke_rect(
            Bounds::new(point(px(0.), px(120.)), size(px(100.), px(50.))),
            PrintStroke::new(px(2.)),
        );
        context.stroke_rounded_rect(
            Bounds::new(point(px(0.), px(180.)), size(px(100.), px(50.))),
            px(10.),
            PrintStroke::new(px(2.)),
        );
        context.stroke_line(
            point(px(10.), px(10.)),
            point(px(40.), px(10.)),
            PrintStroke::new(px(1.)),
        );
        context.draw_text("Hello", point(px(12.), px(24.)), PrintTextStyle::default());
        context.draw_text_block(
            "Wrapped hello world",
            Bounds::new(point(px(12.), px(260.)), size(px(120.), px(48.))),
            PrintTextStyle::default(),
        );

        let image = Arc::new(RenderImage::new(SmallVec::from_elem(
            Frame::new(RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))),
            1,
        )));
        context.draw_image(
            image,
            Bounds::new(point(px(0.), px(320.)), size(px(40.), px(40.))),
        );

        assert_eq!(context.finish().len(), 8);
    }

    #[test]
    fn page_margins_must_leave_content_space() {
        let result = content_size_for_page(size(px(40.), px(40.)), Edges::all(px(20.)));

        assert!(result.is_err());
    }

    #[test]
    fn image_frame_index_is_clamped() {
        let image = Arc::new(RenderImage::new(SmallVec::from_vec(vec![
            Frame::new(RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))),
            Frame::new(RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]))),
        ])));
        let mut context = PrintContext::new(size(px(100.), px(100.)), size(px(80.), px(80.)));

        context.draw_image_with_style(
            image,
            Bounds::new(point(px(0.), px(0.)), size(px(20.), px(20.))),
            PrintImageStyle::new().frame_index(99),
        );

        let commands = context.finish();
        match &commands[0] {
            PrintCommand::Image { style, .. } => assert_eq!(style.selected_frame_index(), 1),
            other => panic!("expected image command, got {other:?}"),
        }
    }
}
