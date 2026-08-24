use super::PlatformPdfSupport;

pub(crate) const SUPPORT: PlatformPdfSupport = PlatformPdfSupport {
    parser_backend: "lopdf-in-memory",
    schematic_preview_backend: "text-and-annotation-bars",
    annotation_backend: "portable-json-bytes",
};
