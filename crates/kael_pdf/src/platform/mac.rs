use super::PlatformPdfSupport;

pub(crate) const SUPPORT: PlatformPdfSupport = PlatformPdfSupport {
    parser_backend: "lopdf",
    schematic_preview_backend: "text-and-annotation-bars",
    annotation_backend: "sidecar-json",
};
