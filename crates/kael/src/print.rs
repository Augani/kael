use crate::{
    App, Bounds, Edges, ObjectFit, Pixels, Point, RenderImage, Rgba, SharedString, Size, point, px,
    rgb, size,
};
use anyhow::{Result, anyhow};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

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

impl PrintDialogMode {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::ShowDialog => "dialog",
            Self::Silent => "silent",
        }
    }
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

    /// Returns a document-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        match self {
            Self::NativeJob { job, mode } => {
                format!("print request native {}, {}", mode.to_text(), job.to_text())
            }
            Self::WebView { .. } => "print request webview hosted document".to_string(),
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

/// Document export formats for native PDF export and save-page
/// flows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentExportFormat {
    /// Export a paged PDF document.
    Pdf,
    /// Save a complete HTML page archive folder plus entry document.
    HtmlComplete,
    /// Save only the top-level HTML document.
    HtmlOnly,
    /// Save a single-file MHTML archive when the browser backend supports it.
    Mhtml,
}

impl DocumentExportFormat {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::HtmlComplete => "html-complete",
            Self::HtmlOnly => "html-only",
            Self::Mhtml => "mhtml",
        }
    }

    fn accepts_extension(self, extension: &str) -> bool {
        match self {
            Self::Pdf => extension.eq_ignore_ascii_case("pdf"),
            Self::HtmlComplete | Self::HtmlOnly => {
                extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
            }
            Self::Mhtml => {
                extension.eq_ignore_ascii_case("mhtml") || extension.eq_ignore_ascii_case("mht")
            }
        }
    }
}

/// Where an exported document should be delivered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentExportDestination {
    /// Return generated bytes to the caller.
    Bytes,
    /// Write generated output to an absolute app-approved file path.
    File(PathBuf),
}

impl DocumentExportDestination {
    /// Create a file destination.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    /// Return true when bytes should be returned to the caller.
    pub fn is_bytes(&self) -> bool {
        matches!(self, Self::Bytes)
    }

    /// Return true when output should be written to a file.
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    /// Return the output path when this destination targets a file.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Bytes => None,
            Self::File(path) => Some(path),
        }
    }

    /// Stable summary text for logs and agent traces.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::File(_) => "file",
        }
    }
}

/// A checked document export request for native print jobs or WebView-hosted
/// documents.
pub enum DocumentExportRequest {
    /// Export a Kael-rendered native print job.
    NativePrintJob {
        /// The native print job to render.
        job: PrintJob,
        /// Output destination.
        destination: DocumentExportDestination,
    },
    /// Export a WebView-hosted document.
    WebView {
        /// The WebView identifier to export.
        id: SharedString,
        /// Export format.
        format: DocumentExportFormat,
        /// Output destination.
        destination: DocumentExportDestination,
    },
}

impl DocumentExportRequest {
    /// Export a native print job to PDF bytes.
    pub fn pdf_bytes(job: PrintJob) -> Self {
        Self::NativePrintJob {
            job,
            destination: DocumentExportDestination::Bytes,
        }
    }

    /// Export a native print job to a PDF file.
    pub fn pdf_file(job: PrintJob, path: impl Into<PathBuf>) -> Self {
        Self::NativePrintJob {
            job,
            destination: DocumentExportDestination::file(path),
        }
    }

    /// Export a WebView-hosted document to PDF bytes.
    pub fn webview_pdf_bytes(id: impl Into<SharedString>) -> Self {
        Self::WebView {
            id: id.into(),
            format: DocumentExportFormat::Pdf,
            destination: DocumentExportDestination::Bytes,
        }
    }

    /// Export a WebView-hosted document to a PDF file.
    pub fn webview_pdf_file(id: impl Into<SharedString>, path: impl Into<PathBuf>) -> Self {
        Self::WebView {
            id: id.into(),
            format: DocumentExportFormat::Pdf,
            destination: DocumentExportDestination::file(path),
        }
    }

    /// Save a WebView-hosted page to an HTML or MHTML file target.
    pub fn webview_save_page(
        id: impl Into<SharedString>,
        format: DocumentExportFormat,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self::WebView {
            id: id.into(),
            format,
            destination: DocumentExportDestination::file(path),
        }
    }

    /// Return true when this request exports a native Kael-rendered job.
    pub fn is_native_job(&self) -> bool {
        matches!(self, Self::NativePrintJob { .. })
    }

    /// Return true when this request exports a WebView-hosted document.
    pub fn is_webview(&self) -> bool {
        matches!(self, Self::WebView { .. })
    }

    /// Return the requested document export format.
    pub fn format(&self) -> DocumentExportFormat {
        match self {
            Self::NativePrintJob { .. } => DocumentExportFormat::Pdf,
            Self::WebView { format, .. } => *format,
        }
    }

    /// Return the output destination.
    pub fn destination(&self) -> &DocumentExportDestination {
        match self {
            Self::NativePrintJob { destination, .. } | Self::WebView { destination, .. } => {
                destination
            }
        }
    }

    /// Return the WebView id when this request targets hosted browser content.
    pub fn webview_id(&self) -> Option<&SharedString> {
        match self {
            Self::WebView { id, .. } => Some(id),
            Self::NativePrintJob { .. } => None,
        }
    }

    /// Returns a document-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "document export request {}, source {}, destination {}",
            self.format().to_text(),
            if self.is_native_job() {
                "native-print-job"
            } else {
                "webview-hosted-document"
            },
            self.destination().to_text()
        )
    }

    /// Validate the export descriptor before rendering bytes or writing files.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::NativePrintJob { job, destination } => {
                job.validate()?;
                validate_document_export_destination(destination, DocumentExportFormat::Pdf)
            }
            Self::WebView {
                id,
                format,
                destination,
            } => {
                validate_webview_print_id(id)?;
                validate_document_export_destination(destination, *format)?;
                if *format != DocumentExportFormat::Pdf && destination.is_bytes() {
                    return Err(anyhow!("save-page exports require a file destination"));
                }
                Ok(())
            }
        }
    }
}

/// The next platform action a checked document output request needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentOutputNextAction {
    /// Render and dispatch a native Kael print job.
    PrintNative,
    /// Ask an existing WebView-hosted document to print.
    PrintHostedDocument,
    /// Render a native Kael print job into PDF output.
    ExportNativePdf,
    /// Export an existing WebView-hosted document to PDF.
    ExportHostedPdf,
    /// Save an existing WebView-hosted page as HTML or MHTML.
    SaveHostedPage,
}

impl DocumentOutputNextAction {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::PrintNative => "print-native",
            Self::PrintHostedDocument => "print-hosted-document",
            Self::ExportNativePdf => "export-native-pdf",
            Self::ExportHostedPdf => "export-hosted-pdf",
            Self::SaveHostedPage => "save-hosted-page",
        }
    }
}

/// A print or export descriptor ready for a document-output implementation.
pub enum DocumentOutputRequest {
    /// Print through a native print job or hosted WebView document.
    Print(PrintRequest),
    /// Export a native print job or hosted WebView document.
    Export(DocumentExportRequest),
}

impl DocumentOutputRequest {
    /// Return true when this handoff describes print dispatch.
    pub fn is_print(&self) -> bool {
        matches!(self, Self::Print(_))
    }

    /// Return true when this handoff describes document export.
    pub fn is_export(&self) -> bool {
        matches!(self, Self::Export(_))
    }

    /// Return true when the output starts from Kael-rendered native content.
    pub fn is_native(&self) -> bool {
        match self {
            Self::Print(request) => request.is_native_job(),
            Self::Export(request) => request.is_native_job(),
        }
    }

    /// Return true when the output starts from an existing WebView-hosted document.
    pub fn is_webview(&self) -> bool {
        match self {
            Self::Print(request) => request.is_webview(),
            Self::Export(request) => request.is_webview(),
        }
    }

    /// Return the next platform action implied by this request.
    pub fn next_action(&self) -> DocumentOutputNextAction {
        match self {
            Self::Print(request) if request.is_native_job() => {
                DocumentOutputNextAction::PrintNative
            }
            Self::Print(_) => DocumentOutputNextAction::PrintHostedDocument,
            Self::Export(request) if request.is_native_job() => {
                DocumentOutputNextAction::ExportNativePdf
            }
            Self::Export(request) if request.format() == DocumentExportFormat::Pdf => {
                DocumentOutputNextAction::ExportHostedPdf
            }
            Self::Export(_) => DocumentOutputNextAction::SaveHostedPage,
        }
    }

    /// Return the print request when this output describes print dispatch.
    pub fn print_request(&self) -> Option<&PrintRequest> {
        match self {
            Self::Print(request) => Some(request),
            Self::Export(_) => None,
        }
    }

    /// Return the export request when this output describes document export.
    pub fn export_request(&self) -> Option<&DocumentExportRequest> {
        match self {
            Self::Export(request) => Some(request),
            Self::Print(_) => None,
        }
    }

    /// Returns a document-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "document output request {}, source {}, next action {}",
            if self.is_print() { "print" } else { "export" },
            if self.is_native() {
                "native"
            } else {
                "webview-hosted"
            },
            self.next_action().to_text()
        )
    }

    /// Validate the output descriptor before dispatching print or export work.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Print(request) => request.validate(),
            Self::Export(request) => request.validate(),
        }
    }
}

/// Builder-facing print/export handoff that validates the output lane before
/// native or WebView work is dispatched.
pub struct DocumentOutputHandoffBuilder {
    request: DocumentOutputRequest,
}

impl DocumentOutputHandoffBuilder {
    /// Build a native print handoff that shows the platform print dialog.
    pub fn print_dialog(job: PrintJob) -> Self {
        Self::from_print_request(PrintRequest::dialog(job))
    }

    /// Build a native print handoff for direct platform printer dispatch.
    pub fn print_silent(job: PrintJob) -> Self {
        Self::from_print_request(PrintRequest::silent(job))
    }

    /// Build a hosted document print handoff by WebView id.
    pub fn print_webview(id: impl Into<SharedString>) -> Self {
        Self::from_print_request(PrintRequest::webview(id))
    }

    /// Build a native PDF export handoff that returns bytes.
    pub fn export_pdf_bytes(job: PrintJob) -> Self {
        Self::from_export_request(DocumentExportRequest::pdf_bytes(job))
    }

    /// Build a native PDF export handoff that writes a file.
    pub fn export_pdf_file(job: PrintJob, path: impl Into<PathBuf>) -> Self {
        Self::from_export_request(DocumentExportRequest::pdf_file(job, path))
    }

    /// Build a hosted PDF export handoff that returns bytes.
    pub fn export_webview_pdf_bytes(id: impl Into<SharedString>) -> Self {
        Self::from_export_request(DocumentExportRequest::webview_pdf_bytes(id))
    }

    /// Build a hosted PDF export handoff that writes a file.
    pub fn export_webview_pdf_file(id: impl Into<SharedString>, path: impl Into<PathBuf>) -> Self {
        Self::from_export_request(DocumentExportRequest::webview_pdf_file(id, path))
    }

    /// Build a hosted save-page handoff for HTML or MHTML output.
    pub fn save_webview_page(
        id: impl Into<SharedString>,
        format: DocumentExportFormat,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self::from_export_request(DocumentExportRequest::webview_save_page(id, format, path))
    }

    /// Wrap an already-built print request.
    pub fn from_print_request(request: PrintRequest) -> Self {
        Self {
            request: DocumentOutputRequest::Print(request),
        }
    }

    /// Wrap an already-built document export request.
    pub fn from_export_request(request: DocumentExportRequest) -> Self {
        Self {
            request: DocumentOutputRequest::Export(request),
        }
    }

    /// Return the next platform action implied by this builder.
    pub fn next_action(&self) -> DocumentOutputNextAction {
        self.request.next_action()
    }

    /// Returns a document-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "document output handoff builder: {}",
            self.request.to_text()
        )
    }

    /// Validate the pending handoff before consuming the builder.
    pub fn validate(&self) -> Result<()> {
        self.request.validate()
    }

    /// Validate and build the document output handoff.
    pub fn build_checked(self) -> Result<DocumentOutputHandoff> {
        self.validate()?;
        let next_action = self.request.next_action();
        Ok(DocumentOutputHandoff {
            request: self.request,
            next_action,
        })
    }
}

/// A checked document output handoff for native print/export and hosted
/// WebView print/export flows.
pub struct DocumentOutputHandoff {
    request: DocumentOutputRequest,
    next_action: DocumentOutputNextAction,
}

impl DocumentOutputHandoff {
    /// Build a checked native print handoff that shows the platform print dialog.
    pub fn print_dialog(job: PrintJob) -> Result<Self> {
        DocumentOutputHandoffBuilder::print_dialog(job).build_checked()
    }

    /// Build a checked native print handoff for direct platform printer dispatch.
    pub fn print_silent(job: PrintJob) -> Result<Self> {
        DocumentOutputHandoffBuilder::print_silent(job).build_checked()
    }

    /// Build a checked hosted document print handoff by WebView id.
    pub fn print_webview(id: impl Into<SharedString>) -> Result<Self> {
        DocumentOutputHandoffBuilder::print_webview(id).build_checked()
    }

    /// Build a checked native PDF export handoff that returns bytes.
    pub fn export_pdf_bytes(job: PrintJob) -> Result<Self> {
        DocumentOutputHandoffBuilder::export_pdf_bytes(job).build_checked()
    }

    /// Build a checked native PDF export handoff that writes a file.
    pub fn export_pdf_file(job: PrintJob, path: impl Into<PathBuf>) -> Result<Self> {
        DocumentOutputHandoffBuilder::export_pdf_file(job, path).build_checked()
    }

    /// Build a checked hosted PDF export handoff that returns bytes.
    pub fn export_webview_pdf_bytes(id: impl Into<SharedString>) -> Result<Self> {
        DocumentOutputHandoffBuilder::export_webview_pdf_bytes(id).build_checked()
    }

    /// Build a checked hosted PDF export handoff that writes a file.
    pub fn export_webview_pdf_file(
        id: impl Into<SharedString>,
        path: impl Into<PathBuf>,
    ) -> Result<Self> {
        DocumentOutputHandoffBuilder::export_webview_pdf_file(id, path).build_checked()
    }

    /// Build a checked hosted save-page handoff for HTML or MHTML output.
    pub fn save_webview_page(
        id: impl Into<SharedString>,
        format: DocumentExportFormat,
        path: impl Into<PathBuf>,
    ) -> Result<Self> {
        DocumentOutputHandoffBuilder::save_webview_page(id, format, path).build_checked()
    }

    /// Return the checked print/export request.
    pub fn request(&self) -> &DocumentOutputRequest {
        &self.request
    }

    /// Return the print request when this handoff describes print dispatch.
    pub fn print_request(&self) -> Option<&PrintRequest> {
        self.request.print_request()
    }

    /// Return the export request when this handoff describes document export.
    pub fn export_request(&self) -> Option<&DocumentExportRequest> {
        self.request.export_request()
    }

    /// Return true when this handoff describes print dispatch.
    pub fn is_print(&self) -> bool {
        self.request.is_print()
    }

    /// Return true when this handoff describes document export.
    pub fn is_export(&self) -> bool {
        self.request.is_export()
    }

    /// Return true when the output starts from Kael-rendered native content.
    pub fn is_native(&self) -> bool {
        self.request.is_native()
    }

    /// Return true when the output starts from an existing WebView-hosted document.
    pub fn is_webview(&self) -> bool {
        self.request.is_webview()
    }

    /// Return the next platform action needed to complete the handoff.
    pub fn next_action(&self) -> DocumentOutputNextAction {
        self.next_action
    }

    /// Returns a document-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!("document output handoff: {}", self.request.to_text())
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

    /// Number of pages configured for this print job.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Return the first configured page size, if any.
    pub fn page_size(&self) -> Option<Size<Pixels>> {
        self.pages.first().map(|page| page.size)
    }

    /// Returns a document-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        let page_size = self
            .page_size()
            .map(|size| format!("{:.0}x{:.0}pt", size.width.0, size.height.0))
            .unwrap_or_else(|| "none".to_string());

        format!(
            "print job: {} pages, {} orientation, page size {page_size}, margins {:.0}/{:.0}/{:.0}/{:.0}pt",
            self.page_count(),
            self.orientation.to_text(),
            self.margins.top.0,
            self.margins.right.0,
            self.margins.bottom.0,
            self.margins.left.0
        )
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

            content_size_for_page(
                oriented_print_page_size(page.size, self.orientation),
                self.margins,
            )?;
        }

        Ok(())
    }

    pub(crate) fn into_platform_job(self, cx: &mut App) -> Result<PlatformPrintJob> {
        self.validate()?;

        let first_page_size = self.pages[0].size;
        let mut rendered_pages = Vec::with_capacity(self.pages.len());

        for page in self.pages {
            let page_size = oriented_print_page_size(page.size, self.orientation);
            let content_size = content_size_for_page(page_size, self.margins)?;
            let mut context = PrintContext::new(page_size, content_size);
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

pub(crate) fn oriented_print_page_size(
    page_size: Size<Pixels>,
    orientation: PrintOrientation,
) -> Size<Pixels> {
    match orientation {
        PrintOrientation::Portrait => page_size,
        PrintOrientation::Landscape => size(page_size.height, page_size.width),
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

fn validate_document_export_destination(
    destination: &DocumentExportDestination,
    format: DocumentExportFormat,
) -> Result<()> {
    match destination {
        DocumentExportDestination::Bytes => Ok(()),
        DocumentExportDestination::File(path) => {
            let path_text = path.to_string_lossy();
            anyhow::ensure!(
                !path_text.trim().is_empty(),
                "document export destination cannot be empty"
            );
            anyhow::ensure!(
                path.is_absolute(),
                "document export destination must be absolute"
            );
            anyhow::ensure!(
                !path_text.chars().any(|ch| ch == '\0'),
                "document export destination cannot contain NUL characters"
            );
            anyhow::ensure!(
                !path
                    .components()
                    .any(|component| { matches!(component, std::path::Component::ParentDir) }),
                "document export destination cannot contain parent-directory components"
            );
            let extension = path.extension().and_then(|extension| extension.to_str());
            anyhow::ensure!(
                extension.is_some_and(|extension| format.accepts_extension(extension)),
                "document export destination extension does not match export format"
            );
            Ok(())
        }
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

    /// Returns a content-safe page summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "print page: size {:.0}x{:.0}pt",
            self.size.width.0, self.size.height.0
        )
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
    /// Stable key for common paper size metadata.
    pub fn key(self) -> &'static str {
        match self {
            Self::Letter => "letter",
            Self::A4 => "a4",
        }
    }

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

impl PrintOrientation {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }
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
        validate_positive_pixels(self.width, "print stroke width")?;
        validate_print_color(self.color, "print stroke color")
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

    /// Validate text style settings before printing.
    pub fn validate(&self) -> Result<()> {
        if let Some(font_family) = &self.font_family {
            validate_print_label(font_family, "print font family", 128)?;
        }
        validate_positive_pixels(self.font_size, "print font size")?;
        validate_print_color(self.color, "print text color")
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
    opacity: f32,
}

impl PrintImageStyle {
    /// Creates an image style with `Contain` fitting and the first frame selected.
    pub fn new() -> Self {
        Self {
            object_fit: PrintImageFit::Contain,
            frame_index: 0,
            opacity: 1.0,
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

    /// Sets image opacity from fully transparent (`0`) to fully opaque (`1`).
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Validate image style settings before printing.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.opacity.is_finite() && (0.0..=1.0).contains(&self.opacity),
            "print image opacity must be finite and between 0 and 1"
        );
        Ok(())
    }

    pub(crate) fn object_fit_ref(&self) -> PrintImageFit {
        self.object_fit
    }

    pub(crate) fn selected_frame_index(&self) -> usize {
        self.frame_index
    }

    pub(crate) fn opacity_ref(&self) -> f32 {
        self.opacity
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

    /// Number of valid print commands recorded so far.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Whether this page has no valid recorded print commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Number of fill commands recorded so far.
    pub fn fill_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    PrintCommand::FillRect { .. } | PrintCommand::FillRoundedRect { .. }
                )
            })
            .count()
    }

    /// Number of stroke commands recorded so far.
    pub fn stroke_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    PrintCommand::StrokeRect { .. }
                        | PrintCommand::StrokeRoundedRect { .. }
                        | PrintCommand::StrokeLine { .. }
                )
            })
            .count()
    }

    /// Number of text commands recorded so far.
    pub fn text_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    PrintCommand::Text { .. } | PrintCommand::TextBlock { .. }
                )
            })
            .count()
    }

    /// Number of image commands recorded so far.
    pub fn image_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|command| matches!(command, PrintCommand::Image { .. }))
            .count()
    }

    /// Returns a content-safe summary of recorded print commands.
    pub fn to_text(&self) -> String {
        format!(
            "print context: {} commands, fills {}, strokes {}, text {}, images {}, page {:.0}x{:.0}pt, content {:.0}x{:.0}pt",
            self.command_count(),
            self.fill_count(),
            self.stroke_count(),
            self.text_count(),
            self.image_count(),
            self.page_size.width.0,
            self.page_size.height.0,
            self.content_size.width.0,
            self.content_size.height.0
        )
    }

    /// Fills a rectangle with a solid color.
    pub fn fill_rect(&mut self, bounds: Bounds<Pixels>, color: impl Into<Rgba>) {
        let color = color.into();
        if validate_print_bounds(bounds, "print fill rectangle").is_err()
            || validate_print_color(color, "print fill color").is_err()
        {
            return;
        }
        self.commands.push(PrintCommand::FillRect { bounds, color });
    }

    /// Fills a rounded rectangle with a solid color.
    pub fn fill_rounded_rect(
        &mut self,
        bounds: Bounds<Pixels>,
        radius: impl Into<Pixels>,
        color: impl Into<Rgba>,
    ) {
        let radius = radius.into();
        let color = color.into();
        if validate_print_bounds(bounds, "print rounded fill rectangle").is_err()
            || validate_non_negative_pixels(radius, "print corner radius").is_err()
            || validate_print_color(color, "print rounded fill color").is_err()
        {
            return;
        }
        self.commands.push(PrintCommand::FillRoundedRect {
            bounds,
            radius,
            color,
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

fn validate_print_color(color: Rgba, label: &str) -> Result<()> {
    let components = [color.r, color.g, color.b, color.a];
    if components
        .into_iter()
        .any(|component| !component.is_finite() || !(0.0..=1.0).contains(&component))
    {
        return Err(anyhow!(
            "{label} components must be finite and between zero and one"
        ));
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

        assert_eq!(context.command_count(), 8);
        assert!(!context.is_empty());
        assert_eq!(context.fill_count(), 2);
        assert_eq!(context.stroke_count(), 3);
        assert_eq!(context.text_count(), 2);
        assert_eq!(context.image_count(), 1);
        assert_eq!(
            context.to_text(),
            "print context: 8 commands, fills 2, strokes 3, text 2, images 1, page 612x792pt, content 540x720pt"
        );
        assert!(!context.to_text().contains("Hello"));
        assert!(!context.to_text().contains("Wrapped hello world"));

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
        assert_eq!(job.page_count(), 1);
        assert_eq!(job.page_size(), Some(size(px(612.), px(792.))));
        assert_eq!(job.pages_ref()[0].size(), size(px(612.), px(792.)));
        assert_eq!(job.pages_ref()[0].to_text(), "print page: size 612x792pt");
        assert_eq!(job.orientation_ref(), PrintOrientation::Portrait);
        assert_eq!(
            job.to_text(),
            "print job: 1 pages, portrait orientation, page size 612x792pt, margins 36/36/36/36pt"
        );
        assert!(!job.to_text().contains("Document"));

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

        assert_eq!(PrintPaperSize::Letter.key(), "letter");
        assert_eq!(PrintPaperSize::A4.key(), "a4");
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
    fn landscape_orientation_rotates_the_render_context_and_margin_validation() {
        let letter = PrintPaperSize::Letter.size();
        assert_eq!(
            oriented_print_page_size(letter, PrintOrientation::Landscape),
            size(px(792.), px(612.))
        );

        let landscape = PrintJob::letter("Landscape", |_, _| {})
            .orientation(PrintOrientation::Landscape)
            .margins(Edges {
                top: px(350.),
                right: px(1.),
                bottom: px(350.),
                left: px(1.),
            });
        assert!(
            landscape.validate().is_err(),
            "landscape margins must be validated against the rotated page height"
        );
    }

    #[test]
    fn print_request_validates_native_and_webview_targets() {
        let dialog = PrintRequest::dialog(PrintJob::letter("Document", |_, _| {}));
        assert!(dialog.validate().is_ok());
        assert!(dialog.is_native_job());
        assert!(!dialog.is_webview());
        assert_eq!(dialog.dialog_mode(), Some(PrintDialogMode::ShowDialog));
        assert_eq!(dialog.dialog_mode().unwrap().to_text(), "dialog");
        assert_eq!(dialog.webview_id(), None);
        assert_eq!(
            dialog.to_text(),
            "print request native dialog, print job: 1 pages, portrait orientation, page size 612x792pt, margins 36/36/36/36pt"
        );
        assert!(!dialog.to_text().contains("Document"));

        let silent = PrintRequest::silent(PrintJob::a4("Receipt", |_, _| {}));
        assert!(silent.validate().is_ok());
        assert_eq!(silent.dialog_mode(), Some(PrintDialogMode::Silent));
        assert_eq!(silent.dialog_mode().unwrap().to_text(), "silent");
        assert!(!silent.to_text().contains("Receipt"));

        let webview = PrintRequest::webview("invoice-preview");
        assert!(webview.validate().is_ok());
        assert!(!webview.is_native_job());
        assert!(webview.is_webview());
        assert_eq!(webview.dialog_mode(), None);
        assert_eq!(webview.webview_id().unwrap().as_ref(), "invoice-preview");
        assert_eq!(webview.to_text(), "print request webview hosted document");
        assert!(!webview.to_text().contains("invoice-preview"));
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
    fn document_export_request_validates_pdf_and_save_page_targets() {
        let native_pdf =
            DocumentExportRequest::pdf_bytes(PrintJob::letter("Secret Report", |_, _| {}));
        assert!(native_pdf.validate().is_ok());
        assert!(native_pdf.is_native_job());
        assert!(!native_pdf.is_webview());
        assert_eq!(native_pdf.format(), DocumentExportFormat::Pdf);
        assert!(native_pdf.destination().is_bytes());
        assert_eq!(
            native_pdf.to_text(),
            "document export request pdf, source native-print-job, destination bytes"
        );
        assert!(!native_pdf.to_text().contains("Secret Report"));

        let native_file =
            DocumentExportRequest::pdf_file(PrintJob::a4("Invoice", |_, _| {}), "/tmp/invoice.pdf");
        assert!(native_file.validate().is_ok());
        assert!(native_file.destination().is_file());
        assert!(!native_file.to_text().contains("/tmp/invoice.pdf"));
        assert!(!native_file.to_text().contains("Invoice"));

        let webview_pdf =
            DocumentExportRequest::webview_pdf_file("invoice-preview", "/tmp/hosted-invoice.pdf");
        assert!(webview_pdf.validate().is_ok());
        assert!(webview_pdf.is_webview());
        assert_eq!(
            webview_pdf.webview_id().unwrap().as_ref(),
            "invoice-preview"
        );
        assert_eq!(webview_pdf.format(), DocumentExportFormat::Pdf);
        assert!(!webview_pdf.to_text().contains("invoice-preview"));
        assert!(!webview_pdf.to_text().contains("hosted-invoice"));

        let save_page = DocumentExportRequest::webview_save_page(
            "docs",
            DocumentExportFormat::HtmlComplete,
            "/tmp/docs.html",
        );
        assert!(save_page.validate().is_ok());
        assert_eq!(save_page.format().to_text(), "html-complete");
        assert!(save_page.destination().path().is_some());
    }

    #[test]
    fn document_export_request_rejects_generated_invalid_values() {
        assert!(
            DocumentExportRequest::pdf_bytes(PrintJob::new("Missing pages"))
                .validate()
                .is_err()
        );
        assert!(
            DocumentExportRequest::pdf_file(PrintJob::letter("Report", |_, _| {}), "report.pdf")
                .validate()
                .is_err()
        );
        assert!(
            DocumentExportRequest::pdf_file(
                PrintJob::letter("Report", |_, _| {}),
                "/tmp/report.txt"
            )
            .validate()
            .is_err()
        );
        assert!(
            DocumentExportRequest::webview_pdf_file("", "/tmp/report.pdf")
                .validate()
                .is_err()
        );
        assert!(
            DocumentExportRequest::webview_pdf_file("report", "/tmp/report.html")
                .validate()
                .is_err()
        );
        assert!(
            DocumentExportRequest::webview_save_page(
                "report",
                DocumentExportFormat::HtmlOnly,
                "/tmp/report.pdf",
            )
            .validate()
            .is_err()
        );

        let invalid_bytes_save = DocumentExportRequest::WebView {
            id: "report".into(),
            format: DocumentExportFormat::Mhtml,
            destination: DocumentExportDestination::Bytes,
        };
        assert!(invalid_bytes_save.validate().is_err());
    }

    #[test]
    fn document_output_handoff_guides_print_and_export_actions() {
        let native_print =
            DocumentOutputHandoffBuilder::print_dialog(PrintJob::letter("Invoice", |_, _| {}));
        assert_eq!(
            native_print.next_action(),
            DocumentOutputNextAction::PrintNative
        );
        assert!(native_print.validate().is_ok());
        assert_eq!(
            native_print.to_text(),
            "document output handoff builder: document output request print, source native, next action print-native"
        );
        assert!(!native_print.to_text().contains("Invoice"));

        let native_print = native_print.build_checked().unwrap();
        assert!(native_print.is_print());
        assert!(!native_print.is_export());
        assert!(native_print.is_native());
        assert!(!native_print.is_webview());
        assert_eq!(
            native_print.next_action(),
            DocumentOutputNextAction::PrintNative
        );
        assert!(native_print.print_request().unwrap().is_native_job());
        assert!(native_print.export_request().is_none());
        assert_eq!(
            native_print.to_text(),
            "document output handoff: document output request print, source native, next action print-native"
        );

        let hosted_print = DocumentOutputHandoff::print_webview("invoice-preview").unwrap();
        assert_eq!(
            hosted_print.next_action(),
            DocumentOutputNextAction::PrintHostedDocument
        );
        assert!(hosted_print.is_print());
        assert!(hosted_print.is_webview());
        assert!(!hosted_print.to_text().contains("invoice-preview"));

        let native_pdf =
            DocumentOutputHandoff::export_pdf_bytes(PrintJob::a4("Report", |_, _| {})).unwrap();
        assert_eq!(
            native_pdf.next_action(),
            DocumentOutputNextAction::ExportNativePdf
        );
        assert!(native_pdf.is_export());
        assert!(native_pdf.is_native());
        assert!(
            native_pdf
                .export_request()
                .unwrap()
                .destination()
                .is_bytes()
        );
        assert!(!native_pdf.to_text().contains("Report"));
    }

    #[test]
    fn document_output_handoff_guides_hosted_pdf_and_save_page_actions() {
        let hosted_pdf =
            DocumentOutputHandoff::export_webview_pdf_file("receipt", "/tmp/receipt.pdf").unwrap();
        assert_eq!(
            hosted_pdf.next_action(),
            DocumentOutputNextAction::ExportHostedPdf
        );
        assert!(hosted_pdf.is_export());
        assert!(hosted_pdf.is_webview());
        assert_eq!(
            hosted_pdf
                .export_request()
                .unwrap()
                .webview_id()
                .unwrap()
                .as_ref(),
            "receipt"
        );
        assert!(!hosted_pdf.to_text().contains("receipt"));
        assert!(!hosted_pdf.to_text().contains("/tmp/receipt.pdf"));

        let save_page = DocumentOutputHandoffBuilder::save_webview_page(
            "docs",
            DocumentExportFormat::Mhtml,
            "/tmp/docs.mhtml",
        );
        assert_eq!(
            save_page.next_action(),
            DocumentOutputNextAction::SaveHostedPage
        );
        assert!(save_page.validate().is_ok());

        let save_page = save_page.build_checked().unwrap();
        assert_eq!(
            save_page.next_action(),
            DocumentOutputNextAction::SaveHostedPage
        );
        assert!(save_page.is_export());
        assert!(save_page.is_webview());
        assert_eq!(
            save_page.export_request().unwrap().format(),
            DocumentExportFormat::Mhtml
        );
        assert!(!save_page.to_text().contains("docs"));
        assert!(!save_page.to_text().contains("/tmp/docs.mhtml"));
    }

    #[test]
    fn document_output_handoff_rejects_invalid_generated_requests() {
        assert!(
            DocumentOutputHandoffBuilder::print_dialog(PrintJob::new("Missing pages"))
                .build_checked()
                .is_err()
        );
        assert!(
            DocumentOutputHandoffBuilder::print_webview(" docs")
                .build_checked()
                .is_err()
        );
        assert!(
            DocumentOutputHandoff::export_pdf_file(
                PrintJob::letter("Report", |_, _| {}),
                "/tmp/report.html",
            )
            .is_err()
        );
        assert!(
            DocumentOutputHandoff::save_webview_page(
                "docs",
                DocumentExportFormat::HtmlOnly,
                "/tmp/docs.pdf",
            )
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
        context.fill_rect(
            Bounds::new(point(px(0.), px(0.)), size(px(10.), px(10.))),
            Rgba {
                r: f32::NAN,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
        context.stroke_line(
            point(px(0.), px(0.)),
            point(px(10.), px(10.)),
            PrintStroke::new(px(1.)).color(Rgba {
                r: 0.0,
                g: 2.0,
                b: 0.0,
                a: 1.0,
            }),
        );

        assert_eq!(
            context.to_text(),
            "print context: 0 commands, fills 0, strokes 0, text 0, images 0, page 612x792pt, content 540x720pt"
        );
        assert!(context.is_empty());
        assert!(context.finish().is_empty());
    }

    #[test]
    fn print_image_opacity_is_bounded() {
        assert!(PrintImageStyle::default().validate().is_ok());
        assert!(PrintImageStyle::new().opacity(0.0).validate().is_ok());
        assert!(PrintImageStyle::new().opacity(0.5).validate().is_ok());
        assert!(PrintImageStyle::new().opacity(1.0).validate().is_ok());
        assert!(PrintImageStyle::new().opacity(-0.01).validate().is_err());
        assert!(PrintImageStyle::new().opacity(1.01).validate().is_err());
        assert!(PrintImageStyle::new().opacity(f32::NAN).validate().is_err());
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
