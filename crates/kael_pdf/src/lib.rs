//! PDF document services for Kael.

#![deny(missing_docs)]

/// PDF annotation types.
pub mod annotation;
/// PDF document loading and persistence.
pub mod document;
/// PDF page access.
pub mod page;
/// Platform metadata for PDF services.
pub mod platform;
/// Raster page preview generation.
pub mod renderer;
/// Text extraction and search helpers.
pub mod text;

pub use anyhow::Result;
pub use annotation::{
    Annotation, AnnotationId, PageAnnotation, PdfColor, PdfPoint, PdfRect, StampKind,
};
pub use document::{OutlineItem, PdfDocument, PdfMetadata};
pub use page::{PdfLink, PdfLinkDestination, PdfPage, PdfPageSize};
pub use renderer::RenderedPage;
pub use text::TextMatch;