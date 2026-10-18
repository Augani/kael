use thiserror::Error;
#[cfg(feature = "dotlottie")]
use zip::result::ZipError;

use crate::support::SupportReport;

/// Errors that can occur while parsing, validating, or rendering Lottie content.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RasterlottieError {
    /// The input JSON could not be deserialized into the library model.
    #[error("failed to parse Lottie JSON: {0}")]
    Parse(#[from] serde_json::Error),

    /// The input `.lottie` archive could not be opened as a ZIP container.
    #[cfg(feature = "dotlottie")]
    #[error("failed to read .lottie archive: {0}")]
    DotLottieArchive(#[from] ZipError),

    /// The input `.lottie` archive is structurally invalid for this loader.
    #[cfg(feature = "dotlottie")]
    #[error("invalid .lottie archive: {detail}")]
    InvalidDotLottie {
        /// Human-readable validation or decode failure reason.
        detail: String,
    },

    /// GIF encoding failed after raster frames had been rendered.
    #[cfg(feature = "gif")]
    #[error("failed to encode GIF: {0}")]
    GifEncoding(#[from] gif::EncodingError),

    /// The internal render pipeline failed unexpectedly.
    #[error("internal render pipeline failure: {detail}")]
    Internal {
        /// Human-readable diagnostic detail for unexpected internal failures.
        detail: String,
    },

    /// The animation uses features that the configured support profile rejects.
    #[error("animation contains unsupported features: {report}")]
    UnsupportedFeatures {
        /// Report that describes the rejected features.
        report: SupportReport,
    },

    /// The requested output canvas size was zero or otherwise invalid.
    #[error("invalid canvas size {width}x{height}")]
    InvalidCanvasSize {
        /// Requested width in pixels.
        width: u32,
        /// Requested height in pixels.
        height: u32,
    },

    /// An image asset was malformed or could not be decoded.
    #[error("invalid image asset `{id}`: {detail}")]
    InvalidImageAsset {
        /// Asset identifier.
        id: String,
        /// Human-readable validation or decode failure reason.
        detail: String,
    },

    /// A text layer could not be interpreted by the text renderer.
    #[error("invalid text layer `{layer}`: {detail}")]
    InvalidTextLayer {
        /// Layer name or identifier.
        layer: String,
        /// Human-readable validation or shaping failure reason.
        detail: String,
    },
}
