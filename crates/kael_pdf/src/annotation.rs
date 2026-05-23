//! PDF annotation types.

use serde::{Deserialize, Serialize};

/// A stable identifier for a page annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnnotationId(pub u64);

/// A point in PDF page space, expressed in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfPoint {
    /// The horizontal position from the left edge.
    pub x: f32,
    /// The vertical position from the bottom edge.
    pub y: f32,
}

impl PdfPoint {
    /// Creates a point in page space.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A rectangle in PDF page space, expressed in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    /// The horizontal origin from the left edge.
    pub x: f32,
    /// The vertical origin from the bottom edge.
    pub y: f32,
    /// The rectangle width in points.
    pub width: f32,
    /// The rectangle height in points.
    pub height: f32,
}

impl PdfRect {
    /// Creates a rectangle in page space.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// A simple RGBA color used by annotations and preview rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfColor {
    /// The red channel.
    pub red: u8,
    /// The green channel.
    pub green: u8,
    /// The blue channel.
    pub blue: u8,
    /// The alpha channel.
    pub alpha: u8,
}

impl PdfColor {
    /// Creates a color from RGBA channels.
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

/// The built-in stamp kinds supported by the sidecar annotation model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StampKind {
    /// An approval stamp.
    Approved,
    /// A draft stamp.
    Draft,
    /// A confidential stamp.
    Confidential,
    /// A caller-provided stamp name.
    Custom(String),
}

/// A page annotation stored by the PDF service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Annotation {
    /// A set of highlighted rectangles.
    Highlight {
        /// The highlighted rectangles.
        rects: Vec<PdfRect>,
        /// The highlight color.
        color: PdfColor,
    },
    /// A note attached at a single point.
    Note {
        /// The note position.
        position: PdfPoint,
        /// The note text.
        text: String,
    },
    /// A free-text box.
    FreeText {
        /// The free-text bounds.
        bounds: PdfRect,
        /// The free-text value.
        text: String,
        /// The font size in points.
        font_size: f32,
    },
    /// An ink path annotation.
    Ink {
        /// The ink paths.
        paths: Vec<Vec<PdfPoint>>,
        /// The stroke color.
        color: PdfColor,
        /// The stroke width in points.
        width: f32,
    },
    /// A stamp annotation.
    Stamp {
        /// The stamp bounds.
        bounds: PdfRect,
        /// The stamp kind.
        kind: StampKind,
    },
}

/// An annotation entry returned for a page, including its identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageAnnotation {
    /// The annotation identifier.
    pub id: AnnotationId,
    /// The annotation payload.
    pub kind: Annotation,
}