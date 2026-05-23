use super::PlatformPdfSupport;

pub(crate) const SUPPORT: PlatformPdfSupport = PlatformPdfSupport {
    parser_backend: "lopdf",
    rendering_backend: "text-preview+pdfkit-planned",
    annotation_backend: "sidecar+pdfkit-planned",
};