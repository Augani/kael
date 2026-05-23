use super::PlatformDocumentSupport;

pub(crate) const SUPPORT: PlatformDocumentSupport = PlatformDocumentSupport {
    recent_documents_backend: "recently-used-xbel+json",
    file_association_backend: "desktop-mime",
    autosave_backend: "xdg-data-or-temp",
};
