use super::PlatformDocumentSupport;

pub(crate) const SUPPORT: PlatformDocumentSupport = PlatformDocumentSupport {
    recent_documents_backend: "jump-list+json",
    file_association_backend: "registry",
    autosave_backend: "localappdata-or-temp",
};