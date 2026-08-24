//! PDF page access.

use anyhow::Result;

use crate::{
    annotation::{Annotation, AnnotationId, PageAnnotation, PdfRect},
    document::PdfDocument,
    renderer::PagePreview,
    text::TextMatch,
};

/// The size of a PDF page, expressed in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfPageSize {
    /// The page width in points.
    pub width: f32,
    /// The page height in points.
    pub height: f32,
}

impl PdfPageSize {
    /// Creates a page size in points.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// The destination for a PDF link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfLinkDestination {
    /// An untrusted URI destination that the application must validate before opening.
    Uri(String),
    /// A named destination.
    Named(String),
    /// A zero-based page index destination.
    Page(usize),
}

/// A link discovered on a PDF page.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfLink {
    /// The link bounds in page space.
    pub bounds: PdfRect,
    /// The link destination.
    pub destination: PdfLinkDestination,
}

/// A handle to a single page within a PDF document.
#[derive(Clone)]
pub struct PdfPage {
    pub(crate) document: PdfDocument,
    pub(crate) page_index: usize,
}

impl PdfPage {
    /// Returns the zero-based page index.
    pub const fn index(&self) -> usize {
        self.page_index
    }

    /// Returns the page size in points.
    pub fn size(&self) -> Result<PdfPageSize> {
        self.document.page_size(self.page_index)
    }

    /// Generates a schematic text-and-annotation preview at the requested scale.
    ///
    /// This is not a rasterization of the PDF page's graphics.
    pub async fn schematic_preview(&self, scale: f32) -> Result<PagePreview> {
        let document = self.document.clone();
        let page_index = self.page_index;
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            smol::unblock(move || document.schematic_preview(page_index, scale)).await
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            document.schematic_preview(page_index, scale)
        }
    }

    /// Returns the extracted text for the page.
    pub fn text(&self) -> Result<String> {
        self.document.page_text(self.page_index)
    }

    /// Searches the extracted page text for a query string.
    pub fn search(&self, query: &str) -> Result<Vec<TextMatch>> {
        self.document.search_page_text(self.page_index, query)
    }

    /// Returns the detected link annotations for the page.
    pub fn links(&self) -> Result<Vec<PdfLink>> {
        self.document.page_links(self.page_index)
    }

    /// Returns the stored annotations for the page.
    pub fn annotations(&self) -> Vec<PageAnnotation> {
        self.document.page_annotations(self.page_index)
    }

    /// Adds an annotation to the page.
    pub fn add_annotation(&self, annotation: Annotation) -> Result<()> {
        self.document
            .add_page_annotation(self.page_index, annotation)
    }

    /// Removes an annotation from the page by identifier.
    pub fn remove_annotation(&self, id: AnnotationId) -> Result<()> {
        self.document.remove_page_annotation(self.page_index, id)
    }
}
