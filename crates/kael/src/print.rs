use crate::{
    App, Bounds, Edges, ObjectFit, Pixels, Point, RenderImage, Rgba, SharedString, Size, point, px,
    rgb, size,
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

/// How a native print job should be dispatched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintDialogMode {
    /// Show the platform print dialog before dispatching the job.
    ShowDialog,
    /// Send the job directly to the platform print system.
    Silent,
}

/// A checked print request for native print jobs or WebView-hosted documents.
pub enum PrintRequest {
    /// Print a Kael-rendered native print job.
    NativeJob {
        /// The native print job to render and dispatch.
        job: PrintJob,
        /// Whether the platform print dialog should be shown.
        mode: PrintDialogMode,
    },
    /// Open the print dialog for a WebView-hosted document.
    WebView {
        /// The WebView identifier to print.
        id: SharedString,
    },
}

impl PrintRequest {
    /// Create a native print request that shows the platform print dialog.
    pub fn dialog(job: PrintJob) -> Self {
        Self::NativeJob {
            job,
            mode: PrintDialogMode::ShowDialog,
        }
    }

    /// Create a native print request that sends directly to the platform print system.
    pub fn silent(job: PrintJob) -> Self {
        Self::NativeJob {
            job,
            mode: PrintDialogMode::Silent,
        }
    }

    /// Create a WebView print request by WebView id.
    pub fn webview(id: impl Into<SharedString>) -> Self {
        Self::WebView { id: id.into() }
    }

    /// Return true when this request prints a native Kael-rendered job.
    pub fn is_native_job(&self) -> bool {
        matches!(self, Self::NativeJob { .. })
    }

    /// Return true when this request prints a WebView-hosted document.
    pub fn is_webview(&self) -> bool {
        matches!(self, Self::WebView { .. })
    }

    /// Return the native print dialog mode when this request owns a native job.
    pub fn dialog_mode(&self) -> Option<PrintDialogMode> {
        match self {
            Self::NativeJob { mode, .. } => Some(*mode),
            Self::WebView { .. } => None,
        }
    }

    /// Return the WebView id when this request targets hosted browser content.
    pub fn webview_id(&self) -> Option<&SharedString> {
        match self {
            Self::WebView { id } => Some(id),
            Self::NativeJob { .. } => None,
        }
    }

    /// Validate the print request before showing native UI or dispatching to a printer.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::NativeJob { job, .. } => job.validate(),
            Self::WebView { id } => validate_webview_print_id(id),
        }
    }
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

    /// Creates a new single-page letter-sized print job.
    pub fn letter(
        title: impl Into<SharedString>,
        render: impl Fn(&mut PrintContext, &mut App) + 'static,
    ) -> Self {
        Self::new(title).page(PrintPage::letter(render))
    }

    /// Creates a new single-page A4 print job.
    pub fn a4(
        title: impl Into<SharedString>,
        render: impl Fn(&mut PrintContext, &mut App) + 'static,
    ) -> Self {
        Self::new(title).page(PrintPage::a4(render))
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

    /// Return the print job title.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// Return the pages configured for this print job.
    pub fn pages_ref(&self) -> &[PrintPage] {
        &self.pages
    }

    /// Return the configured orientation.
    pub fn orientation_ref(&self) -> PrintOrientation {
        self.orientation
    }

    /// Return the configured margins.
    pub fn margins_ref(&self) -> Edges<Pixels> {
        self.margins
    }

    /// Validate the print job before showing OS print UI.
    pub fn validate(&self) -> Result<()> {
        validate_print_title(&self.title)?;

        if self.pages.is_empty() {
            return Err(anyhow!("print jobs must contain at least one page"));
        }

        let first_page_size = self.pages[0].size;
        validate_print_page_size(first_page_size)?;
        validate_print_margins(self.margins)?;

        for page in &self.pages {
            validate_print_page_size(page.size)?;
            if page.size != first_page_size {
                return Err(anyhow!(
                    "all pages in a print job must use the same page size"
                ));
            }

            content_size_for_page(page.size, self.margins)?;
        }

        Ok(())
    }

    pub(crate) fn into_platform_job(self, cx: &mut App) -> Result<PlatformPrintJob> {
        self.validate()?;

        let first_page_size = self.pages[0].size;
        let mut rendered_pages = Vec::with_capacity(self.pages.len());

        for page in self.pages {
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

fn validate_webview_print_id(id: &SharedString) -> Result<()> {
    let id = id.as_ref();
    anyhow::ensure!(!id.trim().is_empty(), "WebView print id cannot be empty");
    anyhow::ensure!(
        id == id.trim(),
        "WebView print id cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !id.chars().any(char::is_control),
        "WebView print id cannot contain control characters"
    );
    Ok(())
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

    /// Creates a US Letter page (8.5 x 11 inches at 72 points per inch).
    pub fn letter(render: impl Fn(&mut PrintContext, &mut App) + 'static) -> Self {
        Self::new(PrintPaperSize::Letter.size(), render)
    }

    /// Creates an A4 page (210 x 297mm at 72 points per inch).
    pub fn a4(render: impl Fn(&mut PrintContext, &mut App) + 'static) -> Self {
        Self::new(PrintPaperSize::A4.size(), render)
    }

    /// Return this page's paper size.
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }
}

/// Common paper sizes expressed in print points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintPaperSize {
    /// US Letter, 8.5 x 11 inches.
    Letter,
    /// A4, 210 x 297 millimeters.
    A4,
}

impl PrintPaperSize {
    /// Return the paper size in print points.
    pub fn size(self) -> Size<Pixels> {
        match self {
            Self::Letter => size(px(612.), px(792.)),
            Self::A4 => size(px(595.), px(842.)),
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

    /// Validate stroke settings before printing.
    pub fn validate(&self) -> Result<()> {
        validate_positive_pixels(self.width, "print stroke width")
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn width(&self) -> Pixels {
        self.width
    }

    #[cfg(target_os = "macos")]
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

    /// Validate text style settings before printing.
    pub fn validate(&self) -> Result<()> {
        if let Some(font_family) = &self.font_family {
            validate_print_label(font_family, "print font family", 128)?;
        }
        validate_positive_pixels(self.font_size, "print font size")
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn font_family_ref(&self) -> Option<&SharedString> {
        self.font_family.as_ref()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn font_size(&self) -> Pixels {
        self.font_size
    }

    #[cfg(target_os = "macos")]
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

    /// Validate image style settings before printing.
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }

    #[cfg(target_os = "macos")]
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
        if validate_print_bounds(bounds, "print fill rectangle").is_err() {
            return;
        }
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
        let radius = radius.into();
        if validate_print_bounds(bounds, "print rounded fill rectangle").is_err()
            || validate_non_negative_pixels(radius, "print corner radius").is_err()
        {
            return;
        }
        self.commands.push(PrintCommand::FillRoundedRect {
            bounds,
            radius,
            color: color.into(),
        });
    }

    /// Strokes a rectangle outline.
    pub fn stroke_rect(&mut self, bounds: Bounds<Pixels>, stroke: PrintStroke) {
        if validate_print_bounds(bounds, "print stroke rectangle").is_err()
            || stroke.validate().is_err()
        {
            return;
        }
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
        let radius = radius.into();
        if validate_print_bounds(bounds, "print rounded stroke rectangle").is_err()
            || validate_non_negative_pixels(radius, "print corner radius").is_err()
            || stroke.validate().is_err()
        {
            return;
        }
        self.commands.push(PrintCommand::StrokeRoundedRect {
            bounds,
            radius,
            stroke,
        });
    }

    /// Draws a single stroked line.
    pub fn stroke_line(&mut self, from: Point<Pixels>, to: Point<Pixels>, stroke: PrintStroke) {
        if validate_print_point(from, "print line start").is_err()
            || validate_print_point(to, "print line end").is_err()
            || stroke.validate().is_err()
        {
            return;
        }
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
        let text = text.into();
        if validate_print_text(&text).is_err()
            || validate_print_point(origin, "print text origin").is_err()
            || style.validate().is_err()
        {
            return;
        }
        self.commands.push(PrintCommand::Text {
            origin,
            text,
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
        let text = text.into();
        if validate_print_text(&text).is_err()
            || validate_print_bounds(bounds, "print text block").is_err()
            || style.validate().is_err()
        {
            return;
        }
        self.commands.push(PrintCommand::TextBlock {
            bounds,
            text,
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
        if validate_print_bounds(bounds, "print image bounds").is_err() || style.validate().is_err()
        {
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct PlatformPrintJob {
    pub(crate) title: SharedString,
    pub(crate) orientation: PrintOrientation,
    pub(crate) margins: Edges<Pixels>,
    pub(crate) page_size: Size<Pixels>,
    pub(crate) pages: Vec<PlatformPrintPage>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

fn validate_print_title(title: &SharedString) -> Result<()> {
    validate_print_label(title, "print job title", 256)
}

fn validate_print_label(value: &SharedString, label: &str, max_len: usize) -> Result<()> {
    let value = value.as_ref();
    if value.trim().is_empty() {
        return Err(anyhow!("{label} cannot be empty"));
    }
    if value != value.trim() {
        return Err(anyhow!(
            "{label} cannot have leading or trailing whitespace"
        ));
    }
    if value.len() > max_len {
        return Err(anyhow!("{label} cannot be longer than {max_len} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(anyhow!("{label} cannot contain control characters"));
    }
    Ok(())
}

fn validate_print_text(text: &SharedString) -> Result<()> {
    if text
        .as_ref()
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(anyhow!("print text cannot contain control characters"));
    }
    Ok(())
}

fn validate_print_page_size(size: Size<Pixels>) -> Result<()> {
    validate_positive_pixels(size.width, "print page width")?;
    validate_positive_pixels(size.height, "print page height")
}

fn validate_print_margins(margins: Edges<Pixels>) -> Result<()> {
    validate_non_negative_pixels(margins.top, "print top margin")?;
    validate_non_negative_pixels(margins.right, "print right margin")?;
    validate_non_negative_pixels(margins.bottom, "print bottom margin")?;
    validate_non_negative_pixels(margins.left, "print left margin")
}

fn validate_print_bounds(bounds: Bounds<Pixels>, label: &str) -> Result<()> {
    validate_print_point(bounds.origin, label)?;
    validate_positive_pixels(bounds.size.width, label)?;
    validate_positive_pixels(bounds.size.height, label)
}

fn validate_print_point(point: Point<Pixels>, label: &str) -> Result<()> {
    validate_finite_pixels(point.x, label)?;
    validate_finite_pixels(point.y, label)
}

fn validate_positive_pixels(value: Pixels, label: &str) -> Result<()> {
    validate_finite_pixels(value, label)?;
    if value.0 <= 0.0 {
        return Err(anyhow!("{label} must be greater than zero"));
    }
    Ok(())
}

fn validate_non_negative_pixels(value: Pixels, label: &str) -> Result<()> {
    validate_finite_pixels(value, label)?;
    if value.0 < 0.0 {
        return Err(anyhow!("{label} cannot be negative"));
    }
    Ok(())
}

fn validate_finite_pixels(value: Pixels, label: &str) -> Result<()> {
    if !value.0.is_finite() {
        return Err(anyhow!("{label} must be finite"));
    }
    Ok(())
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
    fn print_job_validates_title_pages_and_page_size() {
        let page = || PrintPage::new(size(px(612.), px(792.)), |_, _| {});

        let job = PrintJob::new("Document").page(page());
        assert!(job.validate().is_ok());
        assert_eq!(job.title().as_ref(), "Document");
        assert_eq!(job.pages_ref().len(), 1);
        assert_eq!(job.pages_ref()[0].size(), size(px(612.), px(792.)));
        assert_eq!(job.orientation_ref(), PrintOrientation::Portrait);

        assert!(PrintJob::new(" ").page(page()).validate().is_err());
        assert!(PrintJob::new("Document").validate().is_err());
        assert!(
            PrintJob::new("Document")
                .page(page())
                .page(PrintPage::new(size(px(595.), px(842.)), |_, _| {}))
                .validate()
                .is_err()
        );
        assert!(
            PrintJob::new("Document")
                .page(page())
                .margins(Edges::all(px(396.)))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn print_job_supports_named_paper_sizes() {
        let letter = PrintPaperSize::Letter.size();
        let a4 = PrintPaperSize::A4.size();

        assert_eq!(letter, size(px(612.), px(792.)));
        assert_eq!(a4, size(px(595.), px(842.)));

        let letter_job = PrintJob::letter("Letter", |_, _| {});
        let a4_job = PrintJob::a4("A4", |_, _| {});

        assert_eq!(letter_job.pages_ref()[0].size(), letter);
        assert_eq!(a4_job.pages_ref()[0].size(), a4);
        assert!(letter_job.validate().is_ok());
        assert!(a4_job.validate().is_ok());
    }

    #[test]
    fn print_request_validates_native_and_webview_targets() {
        let dialog = PrintRequest::dialog(PrintJob::letter("Document", |_, _| {}));
        assert!(dialog.validate().is_ok());
        assert!(dialog.is_native_job());
        assert!(!dialog.is_webview());
        assert_eq!(dialog.dialog_mode(), Some(PrintDialogMode::ShowDialog));
        assert_eq!(dialog.webview_id(), None);

        let silent = PrintRequest::silent(PrintJob::a4("Receipt", |_, _| {}));
        assert!(silent.validate().is_ok());
        assert_eq!(silent.dialog_mode(), Some(PrintDialogMode::Silent));

        let webview = PrintRequest::webview("invoice-preview");
        assert!(webview.validate().is_ok());
        assert!(!webview.is_native_job());
        assert!(webview.is_webview());
        assert_eq!(webview.dialog_mode(), None);
        assert_eq!(webview.webview_id().unwrap().as_ref(), "invoice-preview");
    }

    #[test]
    fn print_request_rejects_generated_invalid_values() {
        assert!(
            PrintRequest::dialog(PrintJob::new("Missing pages"))
                .validate()
                .is_err()
        );
        assert!(PrintRequest::webview("").validate().is_err());
        assert!(PrintRequest::webview(" invoice").validate().is_err());
        assert!(
            PrintRequest::webview("invoice\npreview")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn print_job_rejects_generated_invalid_values() {
        let page = || PrintPage::letter(|_, _| {});

        assert!(PrintJob::new(" Document").page(page()).validate().is_err());
        assert!(PrintJob::new("Doc\0ument").page(page()).validate().is_err());
        assert!(
            PrintJob::new("Document")
                .page(PrintPage::new(size(px(0.), px(792.)), |_, _| {}))
                .validate()
                .is_err()
        );
        assert!(
            PrintJob::new("Document")
                .page(page())
                .margins(Edges {
                    top: px(-1.),
                    right: px(36.),
                    bottom: px(36.),
                    left: px(36.),
                })
                .validate()
                .is_err()
        );
    }

    #[test]
    fn print_context_rejects_invalid_generated_commands() {
        let mut context = PrintContext::new(size(px(612.), px(792.)), size(px(540.), px(720.)));

        context.fill_rect(
            Bounds::new(point(px(0.), px(0.)), size(px(0.), px(50.))),
            rgb(0xff0000),
        );
        context.stroke_rect(
            Bounds::new(point(px(0.), px(0.)), size(px(100.), px(50.))),
            PrintStroke::new(px(0.)),
        );
        context.draw_text(
            "bad\0text",
            point(px(12.), px(24.)),
            PrintTextStyle::default(),
        );
        context.draw_text(
            "bad font",
            point(px(12.), px(24.)),
            PrintTextStyle::new(px(0.)),
        );

        assert!(context.finish().is_empty());
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
