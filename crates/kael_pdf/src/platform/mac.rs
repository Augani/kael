use super::PlatformPdfSupport;

pub(crate) const SUPPORT: PlatformPdfSupport = PlatformPdfSupport {
    parser_backend: "lopdf",
    rendering_backend: "text-preview",
    annotation_backend: "sidecar-json",
};
