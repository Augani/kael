use super::PlatformDocumentSupport;

pub(crate) const SUPPORT: PlatformDocumentSupport = PlatformDocumentSupport {
    recent_documents_backend: "nsdocumentcontroller+json",
    file_association_backend: "cfbundle-document-types",
    autosave_backend: "library-autosave",
};
