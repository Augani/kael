use super::PlatformDocumentSupport;

pub(crate) const SUPPORT: PlatformDocumentSupport = PlatformDocumentSupport {
    recent_documents_backend: "json",
    file_association_backend: "not-implemented",
    autosave_backend: "configured-path",
};
