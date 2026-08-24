//! PDF document loading and persistence.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::Read as _,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context as _, Result, anyhow};
use lopdf::{Document as LoDocument, Object, ObjectId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    annotation::{Annotation, AnnotationId, PageAnnotation},
    page::{PdfLink, PdfLinkDestination, PdfPage, PdfPageSize},
    renderer::{MAX_PREVIEW_CACHE_BYTES, PagePreview, normalize_scale, render_schematic_preview},
    text::{TextMatch, extract_page_text, search_text},
};

const MAX_PDF_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PDF_OBJECTS: usize = 1_000_000;
const MAX_PDF_PAGES: usize = 100_000;
const MAX_EXTRACTED_PAGE_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXT_CACHE_ENTRIES: usize = 1_024;
const MAX_PREVIEW_CACHE_ENTRIES: usize = 256;
const MAX_ANNOTATION_SIDECAR_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ANNOTATION_MODEL_BYTES: usize = 16 * 1024 * 1024;
const MAX_ANNOTATIONS: usize = 100_000;

/// High-level metadata extracted from a PDF document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdfMetadata {
    /// The document title when present.
    pub title: Option<String>,
    /// The document author when present.
    pub author: Option<String>,
    /// The document subject when present.
    pub subject: Option<String>,
    /// The document creator when present.
    pub creator: Option<String>,
    /// The producing application when present.
    pub producer: Option<String>,
    /// The document keywords.
    pub keywords: Vec<String>,
}

/// A document outline entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutlineItem {
    /// The display title.
    pub title: String,
    /// The zero-based target page index when one is known.
    pub page_index: Option<usize>,
    /// Nested outline items.
    pub children: Vec<OutlineItem>,
}

/// A loaded PDF document.
#[derive(Clone)]
pub struct PdfDocument {
    inner: Arc<Mutex<DocumentState>>,
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    file_operation_lock: Arc<smol::lock::Mutex<()>>,
}

/// A typed error returned when a filesystem-only PDF operation is requested in a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("PDF operation `{operation}` is unavailable in browsers; use the portable byte APIs")]
pub struct PdfPlatformError {
    /// The filesystem-only operation that was requested.
    pub operation: &'static str,
}

#[derive(Debug, Clone)]
struct PageDescriptor {
    page_number: u32,
    object_id: ObjectId,
    size: PdfPageSize,
}

struct DocumentState {
    document: LoDocument,
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    source_path: Option<PathBuf>,
    source_digest: Option<String>,
    pages: Vec<PageDescriptor>,
    page_indices: BTreeMap<ObjectId, usize>,
    metadata: PdfMetadata,
    outline: Vec<OutlineItem>,
    page_text_cache: PageTextCache,
    page_preview_cache: PagePreviewCache,
    annotations: BTreeMap<usize, Arc<Vec<PageAnnotation>>>,
    annotation_load_warning: Option<Arc<str>>,
    annotations_dirty: bool,
    annotation_generation: u64,
    annotation_count: usize,
    annotation_model_bytes: usize,
    next_annotation_id: u64,
}

#[derive(Default)]
struct PageTextCache {
    entries: VecDeque<(usize, Arc<str>)>,
    bytes: usize,
}

impl PageTextCache {
    fn get(&mut self, page_index: usize) -> Option<Arc<str>> {
        let position = self
            .entries
            .iter()
            .position(|(index, _)| *index == page_index)?;
        let entry = self.entries.remove(position)?;
        let text = entry.1.clone();
        self.entries.push_back(entry);
        Some(text)
    }

    fn insert(&mut self, page_index: usize, text: Arc<str>) {
        if let Some(position) = self
            .entries
            .iter()
            .position(|(index, _)| *index == page_index)
            && let Some((_, replaced)) = self.entries.remove(position)
        {
            self.bytes = self.bytes.saturating_sub(replaced.len());
        }
        while self.entries.len() >= MAX_TEXT_CACHE_ENTRIES
            || self.bytes.saturating_add(text.len()) > MAX_TEXT_CACHE_BYTES
        {
            let Some((_, evicted)) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.len());
        }
        if text.len() <= MAX_TEXT_CACHE_BYTES {
            self.bytes = self.bytes.saturating_add(text.len());
            self.entries.push_back((page_index, text));
        }
    }
}

#[derive(Default)]
struct PagePreviewCache {
    entries: VecDeque<((usize, u32, u64), PagePreview)>,
    bytes: usize,
}

impl PagePreviewCache {
    fn get(&mut self, key: (usize, u32, u64)) -> Option<PagePreview> {
        let position = self
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)?;
        let entry = self.entries.remove(position)?;
        let preview = entry.1.clone();
        self.entries.push_back(entry);
        Some(preview)
    }

    fn insert(&mut self, key: (usize, u32, u64), preview: PagePreview) {
        if let Some(position) = self
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)
            && let Some((_, replaced)) = self.entries.remove(position)
        {
            self.bytes = self.bytes.saturating_sub(replaced.pixels().len());
        }
        let preview_bytes = preview.pixels().len();
        while self.entries.len() >= MAX_PREVIEW_CACHE_ENTRIES
            || self.bytes.saturating_add(preview_bytes) > MAX_PREVIEW_CACHE_BYTES
        {
            let Some((_, evicted)) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.pixels().len());
        }
        if preview_bytes <= MAX_PREVIEW_CACHE_BYTES {
            self.bytes = self.bytes.saturating_add(preview_bytes);
            self.entries.push_back((key, preview));
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AnnotationSidecar {
    document_sha256: String,
    pages: BTreeMap<usize, Vec<PageAnnotation>>,
}

impl PdfDocument {
    /// Opens a PDF document from disk.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            let path = normalize_path(path.as_ref())?;
            smol::unblock(move || {
                let data = read_regular_file_bounded(&path, MAX_PDF_BYTES, "PDF document")?;
                let document = LoDocument::load_mem(&data)
                    .with_context(|| format!("failed to open PDF document {}", path.display()))?;
                let digest = digest_hex(&data);
                Self::from_loaded(document, Some(path), Some(digest))
            })
            .await
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let _ = path;
            Err(PdfPlatformError { operation: "open" }.into())
        }
    }

    /// Opens a PDF document from in-memory bytes.
    pub async fn open_from_memory(data: &[u8]) -> Result<Self> {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            ensure_size(
                u64::try_from(data.len()).unwrap_or(u64::MAX),
                MAX_PDF_BYTES,
                "PDF document",
            )?;
            let data = data.to_vec();
            smol::unblock(move || Self::from_bytes(&data)).await
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            Self::from_bytes(data)
        }
    }

    /// Opens a PDF synchronously from bounded in-memory bytes on desktop or in a browser.
    ///
    /// Parsing is CPU work. Browser applications should call this from a Kael web worker when
    /// opening large or untrusted documents so the UI thread remains responsive.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        ensure_size(
            u64::try_from(data.len()).unwrap_or(u64::MAX),
            MAX_PDF_BYTES,
            "PDF document",
        )?;
        let document =
            LoDocument::load_mem(data).context("failed to open PDF document from memory")?;
        Self::from_loaded(document, None, Some(digest_hex(data)))
    }

    /// Returns the number of pages in the document.
    pub fn page_count(&self) -> usize {
        self.inner.lock().pages.len()
    }

    /// Returns a handle to the requested zero-based page index.
    pub fn page(&self, index: usize) -> Result<PdfPage> {
        if index >= self.page_count() {
            return Err(anyhow!("page index {index} out of range"));
        }

        Ok(PdfPage {
            document: self.clone(),
            page_index: index,
        })
    }

    /// Returns the cached document metadata.
    pub fn metadata(&self) -> PdfMetadata {
        self.inner.lock().metadata.clone()
    }

    /// Returns the cached document outline.
    pub fn outline(&self) -> Vec<OutlineItem> {
        self.inner.lock().outline.clone()
    }

    /// Returns a warning produced while loading an invalid annotation sidecar, if any.
    ///
    /// A malformed, stale, or unsafe sidecar never prevents the PDF itself from opening.
    pub fn annotation_load_warning(&self) -> Option<String> {
        self.inner
            .lock()
            .annotation_load_warning
            .as_deref()
            .map(str::to_owned)
    }

    /// Returns whether sidecar annotations have changed since their last successful save.
    pub fn has_unsaved_annotations(&self) -> bool {
        self.inner.lock().annotations_dirty
    }

    /// Saves only the annotation sidecar for a document opened from disk or previously saved.
    ///
    /// The write is refused when the underlying PDF has changed since it was opened or saved.
    pub async fn save_annotations(&self) -> Result<()> {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            return Err(PdfPlatformError {
                operation: "save_annotations",
            }
            .into());
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            let operation_lock = self.file_operation_lock.clone();
            let _operation_guard = operation_lock.lock().await;
            let (path, expected_digest, annotations, page_count, generation) = {
                let state = self.inner.lock();
                (
                    state
                        .source_path
                        .clone()
                        .ok_or_else(|| anyhow!("PDF document has not been saved to a path yet"))?,
                    state
                        .source_digest
                        .clone()
                        .ok_or_else(|| anyhow!("PDF document is missing its source digest"))?,
                    state.annotations.clone(),
                    state.pages.len(),
                    state.annotation_generation,
                )
            };

            smol::unblock(move || {
                let current_digest = digest_file(&path)?;
                anyhow::ensure!(
                    current_digest == expected_digest,
                    "PDF document changed on disk; refusing to save annotations against stale contents"
                );
                persist_annotations(&path, &current_digest, &annotations, page_count)
            })
            .await?;

            let mut state = self.inner.lock();
            state.annotation_load_warning = None;
            if state.annotation_generation == generation {
                state.annotations_dirty = false;
            }
            Ok(())
        }
    }

    /// Saves the current PDF and its sidecar annotations to a destination.
    ///
    /// Prefer [`Self::save_annotations`] when only annotations changed; this method rewrites the
    /// PDF and may invalidate signatures or other byte-sensitive external metadata.
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let _ = path;
            return Err(PdfPlatformError { operation: "save" }.into());
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            let operation_lock = self.file_operation_lock.clone();
            let _operation_guard = operation_lock.lock().await;
            let path = normalize_path(path.as_ref())?;
            let inner = self.inner.clone();

            smol::unblock(move || {
                let mut state = inner.lock();
                write_file_atomically(&path, "PDF document", false, MAX_PDF_BYTES, |file| {
                    state.document.save_to(file).map(|_| ()).map_err(Into::into)
                })
                .with_context(|| format!("failed to save PDF document {}", path.display()))?;
                (|| {
                    let source_digest = digest_file(&path)?;
                    state.source_path = Some(path.clone());
                    state.source_digest = Some(source_digest.clone());
                    persist_annotations(
                        &path,
                        &source_digest,
                        &state.annotations,
                        state.pages.len(),
                    )?;
                    state.annotation_load_warning = None;
                    state.annotations_dirty = false;
                    Ok::<(), anyhow::Error>(())
                })()
                .context("PDF was saved, but its annotation sidecar could not be updated")
            })
            .await
        }
    }

    /// Serializes the current PDF to bounded bytes for download or application-managed storage.
    ///
    /// Rewriting a PDF can invalidate signatures and other byte-sensitive external metadata.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut state = self.inner.lock();
        let mut writer = BoundedVecWriter::new(MAX_PDF_BYTES, "PDF document");
        state
            .document
            .save_to(&mut writer)
            .context("failed to serialize PDF document")?;
        Ok(writer.into_inner())
    }

    /// Serializes the validated sidecar annotation model to portable JSON bytes.
    pub fn annotations_to_bytes(&self) -> Result<Vec<u8>> {
        let state = self.inner.lock();
        let digest = state
            .source_digest
            .as_deref()
            .ok_or_else(|| anyhow!("PDF document is missing its source digest"))?;
        let pages = state
            .annotations
            .iter()
            .map(|(page_index, annotations)| (*page_index, annotations.as_ref().clone()))
            .collect::<BTreeMap<_, _>>();
        validate_annotation_pages(&pages, state.pages.len())?;
        let bytes = serde_json::to_vec(&AnnotationSidecar {
            document_sha256: digest.to_string(),
            pages,
        })
        .context("failed to serialize annotation bytes")?;
        ensure_size(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            MAX_ANNOTATION_SIDECAR_BYTES,
            "PDF annotation bytes",
        )?;
        Ok(bytes)
    }

    /// Replaces annotations from portable sidecar JSON bytes after validating the PDF digest.
    pub fn load_annotations_from_bytes(&self, bytes: &[u8]) -> Result<()> {
        ensure_size(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            MAX_ANNOTATION_SIDECAR_BYTES,
            "PDF annotation bytes",
        )?;
        let sidecar: AnnotationSidecar =
            serde_json::from_slice(bytes).context("failed to deserialize annotation bytes")?;
        validate_digest(&sidecar.document_sha256)?;
        let mut state = self.inner.lock();
        let digest = state
            .source_digest
            .as_deref()
            .ok_or_else(|| anyhow!("PDF document is missing its source digest"))?;
        anyhow::ensure!(
            sidecar.document_sha256 == digest,
            "PDF annotation bytes do not match the document contents"
        );
        validate_annotation_pages(&sidecar.pages, state.pages.len())?;
        let annotations = sidecar
            .pages
            .into_iter()
            .map(|(page_index, entries)| (page_index, Arc::new(entries)))
            .collect::<BTreeMap<_, _>>();
        let (annotation_count, annotation_model_bytes) = annotation_usage(&annotations)?;
        let next_annotation_id = next_annotation_id(&annotations)?;
        state.annotations = annotations;
        state.annotation_count = annotation_count;
        state.annotation_model_bytes = annotation_model_bytes;
        state.next_annotation_id = next_annotation_id;
        state.annotation_generation = state
            .annotation_generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("PDF annotation generation is exhausted"))?;
        state.annotations_dirty = false;
        state.annotation_load_warning = None;
        state.page_preview_cache.clear();
        Ok(())
    }

    pub(crate) fn page_size(&self, page_index: usize) -> Result<PdfPageSize> {
        Ok(self.page_descriptor(page_index)?.size)
    }

    pub(crate) fn page_text(&self, page_index: usize) -> Result<String> {
        Ok(self.page_text_shared(page_index)?.to_string())
    }

    fn page_text_shared(&self, page_index: usize) -> Result<Arc<str>> {
        let mut state = self.inner.lock();
        if let Some(text) = state.page_text_cache.get(page_index) {
            return Ok(text);
        }

        let page_number = state
            .pages
            .get(page_index)
            .ok_or_else(|| anyhow!("page index {page_index} out of range"))?
            .page_number;
        let text = Arc::<str>::from(extract_page_text(&state.document, page_number)?);
        anyhow::ensure!(
            text.len() <= MAX_EXTRACTED_PAGE_TEXT_BYTES,
            "extracted PDF page text exceeds the {MAX_EXTRACTED_PAGE_TEXT_BYTES} byte limit"
        );
        state.page_text_cache.insert(page_index, text.clone());
        Ok(text)
    }

    pub(crate) fn search_page_text(
        &self,
        page_index: usize,
        query: &str,
    ) -> Result<Vec<TextMatch>> {
        let text = self.page_text_shared(page_index)?;
        search_text(page_index, text.as_ref(), query)
    }

    pub(crate) fn schematic_preview(&self, page_index: usize, scale: f32) -> Result<PagePreview> {
        let scale = normalize_scale(scale)?;
        let (page_size, annotation_generation, cached_page) = {
            let mut state = self.inner.lock();
            let page_size = state
                .pages
                .get(page_index)
                .ok_or_else(|| anyhow!("page index {page_index} out of range"))?
                .size;
            let annotation_generation = state.annotation_generation;
            let cached_page =
                state
                    .page_preview_cache
                    .get((page_index, scale.to_bits(), annotation_generation));
            (page_size, annotation_generation, cached_page)
        };

        if let Some(cached_page) = cached_page {
            return Ok(cached_page);
        }

        let text = self.page_text_shared(page_index)?;
        let annotations = self.page_annotations_shared(page_index);
        let rendered = render_schematic_preview(page_size, text.as_ref(), &annotations, scale)?;

        let mut state = self.inner.lock();
        if state.annotation_generation == annotation_generation {
            state.page_preview_cache.insert(
                (page_index, scale.to_bits(), annotation_generation),
                rendered.clone(),
            );
        }

        Ok(rendered)
    }

    pub(crate) fn page_links(&self, page_index: usize) -> Result<Vec<PdfLink>> {
        const MAX_PAGE_LINKS: usize = 10_000;

        let state = self.inner.lock();
        let descriptor = state
            .pages
            .get(page_index)
            .ok_or_else(|| anyhow!("page index {page_index} out of range"))?;
        let mut links = Vec::new();
        for annotation in state.document.get_page_annotations(descriptor.object_id)? {
            if links.len() >= MAX_PAGE_LINKS {
                break;
            }
            if annotation.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Link".as_slice())
            {
                continue;
            }
            let Ok(rect_object) = annotation.get(b"Rect") else {
                continue;
            };
            let Ok(bounds) = link_bounds(&state.document, rect_object) else {
                continue;
            };
            let Some(destination) =
                link_destination(&state.document, annotation, &state.page_indices)
            else {
                continue;
            };
            links.push(PdfLink {
                bounds,
                destination,
            });
        }
        Ok(links)
    }

    pub(crate) fn page_annotations(&self, page_index: usize) -> Vec<PageAnnotation> {
        self.page_annotations_shared(page_index)
            .iter()
            .cloned()
            .collect()
    }

    fn page_annotations_shared(&self, page_index: usize) -> Arc<Vec<PageAnnotation>> {
        self.inner
            .lock()
            .annotations
            .get(&page_index)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn add_page_annotation(
        &self,
        page_index: usize,
        annotation: Annotation,
    ) -> Result<()> {
        let mut state = self.inner.lock();
        let _ = state
            .pages
            .get(page_index)
            .ok_or_else(|| anyhow!("page index {page_index} out of range"))?;
        validate_annotation(&annotation)?;
        let next_generation = state
            .annotation_generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("PDF annotation generation is exhausted"))?;
        anyhow::ensure!(
            state.annotation_count < MAX_ANNOTATIONS,
            "PDF annotation count exceeds the {MAX_ANNOTATIONS} entry limit"
        );
        let next_annotation_id = state
            .next_annotation_id
            .checked_add(1)
            .ok_or_else(|| anyhow!("PDF annotation identifiers are exhausted"))?;
        let entry = PageAnnotation {
            id: AnnotationId(state.next_annotation_id),
            kind: annotation,
        };
        let entry_bytes = annotation_storage_cost(&entry)?;
        let next_model_bytes = state
            .annotation_model_bytes
            .checked_add(entry_bytes)
            .ok_or_else(|| anyhow!("PDF annotation model size overflow"))?;
        anyhow::ensure!(
            next_model_bytes <= MAX_ANNOTATION_MODEL_BYTES,
            "PDF annotation model exceeds the {MAX_ANNOTATION_MODEL_BYTES} byte limit"
        );
        state.next_annotation_id = next_annotation_id;
        Arc::make_mut(state.annotations.entry(page_index).or_default()).push(entry);
        state.annotation_generation = next_generation;
        state.annotation_count += 1;
        state.annotation_model_bytes = next_model_bytes;
        state.annotations_dirty = true;
        state.page_preview_cache.clear();
        Ok(())
    }

    pub(crate) fn remove_page_annotation(&self, page_index: usize, id: AnnotationId) -> Result<()> {
        let mut state = self.inner.lock();
        let next_generation = state
            .annotation_generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("PDF annotation generation is exhausted"))?;
        let annotations = state
            .annotations
            .get(&page_index)
            .ok_or_else(|| anyhow!("page index {page_index} has no annotations"))?;
        let position = annotations
            .iter()
            .position(|annotation| annotation.id == id)
            .ok_or_else(|| anyhow!("annotation {} not found on page {}", id.0, page_index))?;
        let entry_bytes = annotation_storage_cost(&annotations[position])?;
        let next_count = state
            .annotation_count
            .checked_sub(1)
            .ok_or_else(|| anyhow!("PDF annotation count accounting underflow"))?;
        let next_model_bytes = state
            .annotation_model_bytes
            .checked_sub(entry_bytes)
            .ok_or_else(|| anyhow!("PDF annotation size accounting underflow"))?;
        let remove_page = {
            let annotations = state
                .annotations
                .get_mut(&page_index)
                .ok_or_else(|| anyhow!("page index {page_index} has no annotations"))?;
            Arc::make_mut(annotations).remove(position);
            annotations.is_empty()
        };
        if remove_page {
            state.annotations.remove(&page_index);
        }
        state.annotation_generation = next_generation;
        state.annotation_count = next_count;
        state.annotation_model_bytes = next_model_bytes;
        state.annotations_dirty = true;
        state.page_preview_cache.clear();
        Ok(())
    }

    fn from_loaded(
        document: LoDocument,
        source_path: Option<PathBuf>,
        source_digest: Option<String>,
    ) -> Result<Self> {
        anyhow::ensure!(
            document.objects.len() <= MAX_PDF_OBJECTS,
            "PDF document contains more than {MAX_PDF_OBJECTS} objects"
        );
        let pages = collect_pages(&document)?;
        let page_indices = pages
            .iter()
            .enumerate()
            .map(|(index, page)| (page.object_id, index))
            .collect::<BTreeMap<_, _>>();
        let metadata = extract_metadata(&document);
        let outline = extract_outline(&document, &page_indices).unwrap_or_default();
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let (annotations, annotation_load_warning) = if let Some(path) = source_path.as_ref() {
            let source_digest = source_digest
                .as_deref()
                .ok_or_else(|| anyhow!("missing PDF source digest"))?;
            match load_annotations(path, source_digest, pages.len()) {
                Ok(annotations) => (annotations, None),
                Err(error) => (
                    BTreeMap::new(),
                    Some(Arc::<str>::from(format!("{error:#}"))),
                ),
            }
        } else {
            (BTreeMap::new(), None)
        };
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let (annotations, annotation_load_warning) = {
            let _ = source_path;
            (BTreeMap::new(), None)
        };
        let (annotation_count, annotation_model_bytes) = annotation_usage(&annotations)?;
        let next_annotation_id = next_annotation_id(&annotations)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(DocumentState {
                document,
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                source_path,
                source_digest,
                pages,
                page_indices,
                metadata,
                outline,
                page_text_cache: PageTextCache::default(),
                page_preview_cache: PagePreviewCache::default(),
                annotations,
                annotation_load_warning,
                annotations_dirty: false,
                annotation_generation: 0,
                annotation_count,
                annotation_model_bytes,
                next_annotation_id,
            })),
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            file_operation_lock: Arc::new(smol::lock::Mutex::new(())),
        })
    }

    fn page_descriptor(&self, page_index: usize) -> Result<PageDescriptor> {
        self.inner
            .lock()
            .pages
            .get(page_index)
            .cloned()
            .ok_or_else(|| anyhow!("page index {page_index} out of range"))
    }
}

fn collect_pages(document: &LoDocument) -> Result<Vec<PageDescriptor>> {
    let document_pages = document.get_pages();
    anyhow::ensure!(
        document_pages.len() <= MAX_PDF_PAGES,
        "PDF document contains more than {MAX_PDF_PAGES} pages"
    );
    let mut pages = Vec::new();
    for (page_number, object_id) in document_pages {
        pages.push(PageDescriptor {
            page_number,
            object_id,
            size: page_size_for_object(document, object_id)?,
        });
    }
    Ok(pages)
}

fn ensure_size(size: u64, limit: u64, label: &str) -> Result<()> {
    anyhow::ensure!(
        size <= limit,
        "{label} is {size} bytes, exceeding the {limit} byte limit"
    );
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn read_regular_file_bounded(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>> {
    let path_metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {label} {}", path.display()))?;
    anyhow::ensure!(
        path_metadata.file_type().is_file(),
        "{label} {} is not a regular file",
        path.display()
    );
    ensure_size(path_metadata.len(), limit, label)?;

    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("failed to open {label} {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect opened {label} {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file(),
        "{label} {} is not a regular file",
        path.display()
    );
    ensure_size(metadata.len(), limit, label)?;

    let capacity = usize::try_from(metadata.len())
        .with_context(|| format!("{label} is too large for this platform's address space"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .with_context(|| format!("failed to reserve {label} buffer"))?;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .with_context(|| format!("failed to read {label} {}", path.display()))?;
        if read == 0 {
            break;
        }
        let next_len = bytes
            .len()
            .checked_add(read)
            .ok_or_else(|| anyhow!("{label} size overflow"))?;
        ensure_size(u64::try_from(next_len).unwrap_or(u64::MAX), limit, label)?;
        bytes
            .try_reserve_exact(read)
            .with_context(|| format!("failed to grow {label} buffer"))?;
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(bytes)
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn digest_file(path: &Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect saved PDF document {}", path.display()))?;
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "saved PDF document {} is not a regular file",
        path.display()
    );
    ensure_size(metadata.len(), MAX_PDF_BYTES, "saved PDF document")?;
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to hash saved PDF document {}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("failed to hash saved PDF document {}", path.display()))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or_else(|| anyhow!("saved PDF document size overflow"))?;
        ensure_size(total, MAX_PDF_BYTES, "saved PDF document")?;
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_digest(digest: &str) -> Result<()> {
    anyhow::ensure!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "invalid PDF SHA-256 digest"
    );
    Ok(())
}

fn page_size_for_object(document: &LoDocument, object_id: ObjectId) -> Result<PdfPageSize> {
    if let Some(media_box) = media_box_for_object(document, object_id)? {
        let numbers = media_box
            .iter()
            .map(|value| object_number(value))
            .collect::<Result<Vec<_>>>()?;
        anyhow::ensure!(numbers.len() == 4, "PDF MediaBox must contain four numbers");
        let width = (numbers[2] - numbers[0]).abs();
        let height = (numbers[3] - numbers[1]).abs();
        const MAX_PAGE_POINTS: f32 = 200_000.0;
        anyhow::ensure!(
            width.is_finite()
                && height.is_finite()
                && width > 0.0
                && height > 0.0
                && width <= MAX_PAGE_POINTS
                && height <= MAX_PAGE_POINTS,
            "PDF MediaBox must contain finite positive dimensions no larger than {MAX_PAGE_POINTS} points"
        );
        return Ok(PdfPageSize::new(width, height));
    }

    Ok(PdfPageSize::new(612.0, 792.0))
}

fn media_box_for_object(document: &LoDocument, object_id: ObjectId) -> Result<Option<Vec<Object>>> {
    let mut current = object_id;
    let mut visited = BTreeSet::new();
    loop {
        anyhow::ensure!(
            visited.insert(current),
            "cycle in PDF page parent hierarchy"
        );
        let dictionary = document.get_object(current)?.as_dict()?;
        if let Ok(media_box) = dictionary.get(b"MediaBox") {
            return Ok(Some(resolve_array(document, media_box)?));
        }
        match dictionary.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent_id) => current = parent_id,
            Err(_) => return Ok(None),
        }
    }
}

fn resolve_array(document: &LoDocument, object: &Object) -> Result<Vec<Object>> {
    let mut current = object;
    let mut visited = BTreeSet::new();
    for _ in 0..=128 {
        match current {
            Object::Array(values) => return Ok(values.clone()),
            Object::Reference(object_id) => {
                anyhow::ensure!(visited.insert(*object_id), "cycle in PDF array references");
                current = document
                    .objects
                    .get(object_id)
                    .ok_or_else(|| anyhow!("missing PDF object {object_id:?}"))?;
            }
            _ => return Err(anyhow!("expected PDF array object")),
        }
    }
    Err(anyhow!("PDF array reference limit exceeded"))
}

fn object_number(object: &Object) -> Result<f32> {
    match object {
        Object::Integer(value) => Ok(*value as f32),
        Object::Real(value) => Ok(*value),
        _ => Err(anyhow!("expected numeric PDF object")),
    }
}

fn link_bounds(document: &LoDocument, object: &Object) -> Result<crate::annotation::PdfRect> {
    let values = resolve_array(document, object)?;
    anyhow::ensure!(values.len() == 4, "PDF link Rect must contain four numbers");
    let values = values
        .iter()
        .map(object_number)
        .collect::<Result<Vec<_>>>()?;
    anyhow::ensure!(
        values.iter().all(|value| value.is_finite()),
        "PDF link Rect contains non-finite coordinates"
    );
    Ok(crate::annotation::PdfRect::new(
        values[0].min(values[2]),
        values[1].min(values[3]),
        (values[2] - values[0]).abs(),
        (values[3] - values[1]).abs(),
    ))
}

fn link_destination(
    document: &LoDocument,
    annotation: &lopdf::Dictionary,
    page_indices: &BTreeMap<ObjectId, usize>,
) -> Option<PdfLinkDestination> {
    if let Ok(action) = annotation.get(b"A") {
        let action = resolve_dictionary(document, action).ok()?;
        match action.get(b"S").and_then(Object::as_name).ok()? {
            b"URI" => {
                return action
                    .get(b"URI")
                    .ok()
                    .and_then(|object| bounded_destination_text(document, object))
                    .map(PdfLinkDestination::Uri);
            }
            b"GoTo" => {
                return action.get(b"D").ok().and_then(|destination| {
                    parse_destination(document, destination, page_indices)
                });
            }
            _ => {}
        }
    }
    annotation
        .get(b"Dest")
        .ok()
        .and_then(|destination| parse_destination(document, destination, page_indices))
}

fn resolve_dictionary<'a>(
    document: &'a LoDocument,
    object: &'a Object,
) -> Result<&'a lopdf::Dictionary> {
    let (_, object) = document.dereference(object)?;
    object.as_dict().map_err(Into::into)
}

fn parse_destination(
    document: &LoDocument,
    object: &Object,
    page_indices: &BTreeMap<ObjectId, usize>,
) -> Option<PdfLinkDestination> {
    parse_destination_inner(document, object, page_indices, 0)
}

fn parse_destination_inner(
    document: &LoDocument,
    object: &Object,
    page_indices: &BTreeMap<ObjectId, usize>,
    depth: usize,
) -> Option<PdfLinkDestination> {
    const MAX_DESTINATION_DEPTH: usize = 128;
    if depth > MAX_DESTINATION_DEPTH {
        return None;
    }
    match object {
        Object::Name(_) | Object::String(_, _) => {
            bounded_destination_text(document, object).map(PdfLinkDestination::Named)
        }
        Object::Reference(object_id) => page_indices
            .get(object_id)
            .copied()
            .map(PdfLinkDestination::Page),
        Object::Array(values) => values
            .first()
            .and_then(|target| parse_destination_inner(document, target, page_indices, depth + 1)),
        _ => document
            .dereference(object)
            .ok()
            .and_then(|(_, target)| (!std::ptr::eq(target, object)).then_some(target))
            .and_then(|target| parse_destination_inner(document, target, page_indices, depth + 1)),
    }
}

fn bounded_destination_text(document: &LoDocument, object: &Object) -> Option<String> {
    const MAX_DESTINATION_BYTES: usize = 16 * 1024;
    let (_, resolved) = document.dereference(object).ok()?;
    let bytes = match resolved {
        Object::String(bytes, _) | Object::Name(bytes) => bytes,
        _ => return None,
    };
    if bytes.len() > MAX_DESTINATION_BYTES || bytes.iter().any(u8::is_ascii_control) {
        return None;
    }
    bounded_pdf_string(document, object, MAX_DESTINATION_BYTES)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
}

fn extract_metadata(document: &LoDocument) -> PdfMetadata {
    const MAX_METADATA_FIELD_BYTES: usize = 64 * 1024;
    const MAX_KEYWORDS: usize = 1_024;

    let mut metadata = PdfMetadata::default();
    let Some(info_id) = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| object.as_reference().ok())
    else {
        return metadata;
    };

    let Ok(dictionary) = document.get_object(info_id).and_then(Object::as_dict) else {
        return metadata;
    };

    let bounded_field = |key: &[u8]| {
        dictionary
            .get(key)
            .ok()
            .and_then(|object| bounded_pdf_string(document, object, MAX_METADATA_FIELD_BYTES))
    };
    metadata.title = bounded_field(b"Title");
    metadata.author = bounded_field(b"Author");
    metadata.subject = bounded_field(b"Subject");
    metadata.creator = bounded_field(b"Creator");
    metadata.producer = bounded_field(b"Producer");
    metadata.keywords = dictionary
        .get(b"Keywords")
        .ok()
        .and_then(|object| bounded_pdf_string(document, object, MAX_METADATA_FIELD_BYTES))
        .map(|keywords| {
            keywords
                .split(|character| matches!(character, ',' | ';'))
                .map(str::trim)
                .filter(|keyword| !keyword.is_empty())
                .take(MAX_KEYWORDS)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    metadata
}

fn extract_outline(
    document: &LoDocument,
    page_indices: &BTreeMap<ObjectId, usize>,
) -> Result<Vec<OutlineItem>> {
    let root = document.trailer.get(b"Root")?;
    let catalog = resolve_dictionary(document, root)?;
    let outlines = match catalog.get(b"Outlines") {
        Ok(outlines) => resolve_dictionary(document, outlines)?,
        Err(_) => return Ok(Vec::new()),
    };
    let first = match outlines.get(b"First") {
        Ok(first) => first.clone(),
        Err(_) => return Ok(Vec::new()),
    };
    let mut visited = BTreeSet::new();
    let mut total = 0usize;
    walk_outline_siblings(document, first, page_indices, 0, &mut visited, &mut total)
}

fn walk_outline_siblings(
    document: &LoDocument,
    mut current: Object,
    page_indices: &BTreeMap<ObjectId, usize>,
    depth: usize,
    visited: &mut BTreeSet<ObjectId>,
    total: &mut usize,
) -> Result<Vec<OutlineItem>> {
    const MAX_OUTLINE_DEPTH: usize = 128;
    const MAX_OUTLINE_ITEMS: usize = 10_000;
    anyhow::ensure!(
        depth <= MAX_OUTLINE_DEPTH,
        "PDF outline nesting is too deep"
    );

    let mut items = Vec::new();
    loop {
        if let Object::Reference(object_id) = &current {
            anyhow::ensure!(visited.insert(*object_id), "cycle in PDF outline");
        }
        *total = total
            .checked_add(1)
            .ok_or_else(|| anyhow!("PDF outline item count overflow"))?;
        anyhow::ensure!(
            *total <= MAX_OUTLINE_ITEMS,
            "PDF outline contains more than {MAX_OUTLINE_ITEMS} items"
        );
        let dictionary = resolve_dictionary(document, &current)?;
        let title = dictionary
            .get(b"Title")
            .ok()
            .and_then(|object| bounded_pdf_string(document, object, 4 * 1024))
            .unwrap_or_default();
        let page_index =
            link_destination(document, dictionary, page_indices).and_then(|destination| {
                match destination {
                    PdfLinkDestination::Page(index) => Some(index),
                    PdfLinkDestination::Uri(_) | PdfLinkDestination::Named(_) => None,
                }
            });
        let children = match dictionary.get(b"First") {
            Ok(first) => walk_outline_siblings(
                document,
                first.clone(),
                page_indices,
                depth + 1,
                visited,
                total,
            )?,
            Err(_) => Vec::new(),
        };
        if !title.is_empty() {
            items.push(OutlineItem {
                title,
                page_index,
                children,
            });
        }
        match dictionary.get(b"Next") {
            Ok(next) => current = next.clone(),
            Err(_) => break,
        }
    }
    Ok(items)
}

fn bounded_pdf_string(document: &LoDocument, object: &Object, max_bytes: usize) -> Option<String> {
    let (_, object) = document.dereference(object).ok()?;
    let bytes = match object {
        Object::String(bytes, _) | Object::Name(bytes) if bytes.len() <= max_bytes => bytes,
        _ => return None,
    };
    let value = match object {
        Object::String(_, _) => lopdf::decode_text_string(object)
            .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned()),
        Object::Name(_) => String::from_utf8_lossy(bytes).into_owned(),
        _ => return None,
    };
    (value.len() <= max_bytes).then_some(value)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn load_annotations(
    path: &Path,
    document_sha256: &str,
    page_count: usize,
) -> Result<BTreeMap<usize, Arc<Vec<PageAnnotation>>>> {
    let sidecar_path = annotations_sidecar_path(path);
    match std::fs::symlink_metadata(&sidecar_path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to inspect annotation sidecar {}",
                    sidecar_path.display()
                )
            });
        }
    }

    let json = read_regular_file_bounded(
        &sidecar_path,
        MAX_ANNOTATION_SIDECAR_BYTES,
        "PDF annotation sidecar",
    )?;
    let sidecar: AnnotationSidecar =
        serde_json::from_slice(&json).context("failed to deserialize annotation sidecar")?;
    validate_digest(&sidecar.document_sha256)?;
    anyhow::ensure!(
        sidecar.document_sha256 == document_sha256,
        "PDF annotation sidecar does not match the document contents"
    );
    validate_annotation_pages(&sidecar.pages, page_count)?;
    Ok(sidecar
        .pages
        .into_iter()
        .map(|(page_index, annotations)| (page_index, Arc::new(annotations)))
        .collect())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn persist_annotations(
    path: &Path,
    document_sha256: &str,
    annotations: &BTreeMap<usize, Arc<Vec<PageAnnotation>>>,
    page_count: usize,
) -> Result<()> {
    let sidecar_path = annotations_sidecar_path(path);
    if annotations.is_empty() {
        return remove_regular_file_if_exists(&sidecar_path, "PDF annotation sidecar");
    }

    let pages = annotations
        .iter()
        .map(|(page_index, annotations)| (*page_index, annotations.as_ref().clone()))
        .collect();
    validate_annotation_pages(&pages, page_count)?;
    validate_digest(document_sha256)?;
    let json = serde_json::to_vec(&AnnotationSidecar {
        document_sha256: document_sha256.to_string(),
        pages,
    })
    .context("failed to serialize annotation sidecar")?;
    ensure_size(
        u64::try_from(json.len()).unwrap_or(u64::MAX),
        MAX_ANNOTATION_SIDECAR_BYTES,
        "PDF annotation sidecar",
    )?;
    write_bytes_atomically(&sidecar_path, &json)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn remove_regular_file_if_exists(path: &Path, label: &str) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_file(),
                "{label} {} is not a regular file",
                path.display()
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {label} {}", path.display()));
        }
    }
    std::fs::remove_file(path)
        .with_context(|| format!("failed to remove {label} {}", path.display()))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn annotations_sidecar_path(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("document.pdf"))
        .to_os_string();
    file_name.push(".annotations.json");
    path.with_file_name(file_name)
}

fn next_annotation_id(annotations: &BTreeMap<usize, Arc<Vec<PageAnnotation>>>) -> Result<u64> {
    annotations
        .values()
        .flat_map(|entries| entries.iter().map(|entry| entry.id.0))
        .max()
        .map_or(Ok(1), |value| {
            value
                .checked_add(1)
                .ok_or_else(|| anyhow!("PDF annotation identifiers are exhausted"))
        })
}

fn annotation_usage(
    annotations: &BTreeMap<usize, Arc<Vec<PageAnnotation>>>,
) -> Result<(usize, usize)> {
    let mut count = 0usize;
    let mut bytes = 0usize;
    for annotation in annotations.values().flat_map(|entries| entries.iter()) {
        count = count
            .checked_add(1)
            .ok_or_else(|| anyhow!("PDF annotation count overflow"))?;
        bytes = bytes
            .checked_add(annotation_storage_cost(annotation)?)
            .ok_or_else(|| anyhow!("PDF annotation model size overflow"))?;
    }
    anyhow::ensure!(
        count <= MAX_ANNOTATIONS,
        "PDF annotation count exceeds the {MAX_ANNOTATIONS} entry limit"
    );
    anyhow::ensure!(
        bytes <= MAX_ANNOTATION_MODEL_BYTES,
        "PDF annotation model exceeds the {MAX_ANNOTATION_MODEL_BYTES} byte limit"
    );
    Ok((count, bytes))
}

fn annotation_storage_cost(annotation: &PageAnnotation) -> Result<usize> {
    const ENTRY_OVERHEAD_BYTES: usize = 64;

    let mut counter = CountingWriter::default();
    serde_json::to_writer(&mut counter, annotation)
        .context("failed to measure PDF annotation storage")?;
    counter
        .bytes
        .checked_add(ENTRY_OVERHEAD_BYTES)
        .ok_or_else(|| anyhow!("PDF annotation model size overflow"))
}

fn validate_annotation_pages(
    pages: &BTreeMap<usize, Vec<PageAnnotation>>,
    page_count: usize,
) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut total = 0usize;
    for (page_index, annotations) in pages {
        anyhow::ensure!(
            *page_index < page_count,
            "annotation sidecar page index {page_index} is out of range"
        );
        total = total
            .checked_add(annotations.len())
            .ok_or_else(|| anyhow!("PDF annotation count overflow"))?;
        anyhow::ensure!(
            total <= MAX_ANNOTATIONS,
            "PDF annotation count exceeds the {MAX_ANNOTATIONS} entry limit"
        );
        for annotation in annotations {
            anyhow::ensure!(
                annotation.id.0 != 0 && ids.insert(annotation.id),
                "PDF annotation identifiers must be non-zero and unique"
            );
            validate_annotation(&annotation.kind)?;
        }
    }
    Ok(())
}

fn validate_annotation(annotation: &Annotation) -> Result<()> {
    const MAX_TEXT_BYTES: usize = 1024 * 1024;
    const MAX_RECTS: usize = 10_000;
    const MAX_INK_PATHS: usize = 10_000;
    const MAX_INK_POINTS: usize = 1_000_000;

    match annotation {
        Annotation::Highlight { rects, .. } => {
            anyhow::ensure!(
                rects.len() <= MAX_RECTS,
                "PDF highlight has too many rectangles"
            );
            for rect in rects {
                validate_rect(*rect)?;
            }
        }
        Annotation::Note { position, text } => {
            validate_point(*position)?;
            anyhow::ensure!(text.len() <= MAX_TEXT_BYTES, "PDF note text is too large");
        }
        Annotation::FreeText {
            bounds,
            text,
            font_size,
        } => {
            validate_rect(*bounds)?;
            anyhow::ensure!(text.len() <= MAX_TEXT_BYTES, "PDF free text is too large");
            anyhow::ensure!(
                font_size.is_finite() && *font_size > 0.0 && *font_size <= 10_000.0,
                "PDF annotation font size must be finite and positive"
            );
        }
        Annotation::Ink { paths, width, .. } => {
            anyhow::ensure!(
                paths.len() <= MAX_INK_PATHS,
                "PDF ink annotation has too many paths"
            );
            anyhow::ensure!(
                width.is_finite() && *width > 0.0 && *width <= 10_000.0,
                "PDF ink width must be finite and positive"
            );
            let mut total_points = 0usize;
            for path in paths {
                total_points = total_points
                    .checked_add(path.len())
                    .ok_or_else(|| anyhow!("PDF ink point count overflow"))?;
                anyhow::ensure!(
                    total_points <= MAX_INK_POINTS,
                    "PDF ink annotation has too many points"
                );
                for point in path {
                    validate_point(*point)?;
                }
            }
        }
        Annotation::Stamp { bounds, kind } => {
            validate_rect(*bounds)?;
            if let crate::annotation::StampKind::Custom(name) = kind {
                anyhow::ensure!(name.len() <= 4_096, "custom PDF stamp name is too large");
            }
        }
    }
    Ok(())
}

fn validate_point(point: crate::annotation::PdfPoint) -> Result<()> {
    const MAX_COORDINATE: f32 = 10_000_000.0;
    anyhow::ensure!(
        point.x.is_finite()
            && point.y.is_finite()
            && point.x.abs() <= MAX_COORDINATE
            && point.y.abs() <= MAX_COORDINATE,
        "PDF annotation point must contain finite bounded coordinates"
    );
    Ok(())
}

fn validate_rect(rect: crate::annotation::PdfRect) -> Result<()> {
    validate_point(crate::annotation::PdfPoint::new(rect.x, rect.y))?;
    anyhow::ensure!(
        rect.width.is_finite()
            && rect.height.is_finite()
            && rect.width > 0.0
            && rect.height > 0.0
            && rect.width <= 10_000_000.0
            && rect.height <= 10_000_000.0,
        "PDF annotation rectangle must have finite positive bounded dimensions"
    );
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    write_file_atomically(
        path,
        "PDF annotation sidecar",
        true,
        MAX_ANNOTATION_SIDECAR_BYTES,
        |file| file.write_all(bytes).map_err(Into::into),
    )
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len())
            .ok_or_else(|| std::io::Error::other("counted byte length overflow"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct BoundedVecWriter {
    bytes: Vec<u8>,
    limit: u64,
    label: &'static str,
}

impl BoundedVecWriter {
    fn new(limit: u64, label: &'static str) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            label,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for BoundedVecWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let next_len = self
            .bytes
            .len()
            .checked_add(buffer.len())
            .ok_or_else(|| std::io::Error::other("PDF byte length overflow"))?;
        if u64::try_from(next_len).unwrap_or(u64::MAX) > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!("{} exceeds its configured byte limit", self.label),
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
struct BoundedFileWriter<'a> {
    file: &'a mut std::fs::File,
    remaining: u64,
    label: &'a str,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl Write for BoundedFileWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!("{} exceeds its configured byte limit", self.label),
            ));
        }
        let allowed = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let written = self.file.write(&buffer[..allowed])?;
        let written_u64 = u64::try_from(written)
            .map_err(|_| std::io::Error::other(format!("{} write length overflow", self.label)))?;
        self.remaining = self.remaining.checked_sub(written_u64).ok_or_else(|| {
            std::io::Error::other(format!("{} write accounting underflow", self.label))
        })?;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn write_file_atomically(
    path: &Path,
    label: &str,
    private: bool,
    max_bytes: u64,
    write: impl FnOnce(&mut BoundedFileWriter<'_>) -> Result<()>,
) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {label} directory {}", parent.display()))?;
        let metadata = std::fs::symlink_metadata(parent)
            .with_context(|| format!("failed to inspect {label} directory {}", parent.display()))?;
        anyhow::ensure!(
            metadata.file_type().is_dir(),
            "{label} directory {} is not a real directory",
            parent.display()
        );
    }

    let existing_permissions = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file(),
                "{label} target {} is not a regular file",
                path.display()
            );
            Some(metadata.permissions())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {label} target {}", path.display()));
        }
    };

    let (temp_path, mut file) = create_temporary_file(path, label)?;
    if let Err(error) = configure_temporary_permissions(&file, existing_permissions, private) {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to preserve permissions for temporary {label} {}",
                temp_path.display()
            )
        });
    }
    let write_result = (|| {
        {
            let mut writer = BoundedFileWriter {
                file: &mut file,
                remaining: max_bytes,
                label,
            };
            write(&mut writer)?;
            writer.flush()?;
        }
        file.sync_all()?;
        Ok::<(), anyhow::Error>(())
    })();
    if let Err(error) = write_result {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(error)
            .with_context(|| format!("failed to write temporary {label} {}", temp_path.display()));
    }
    drop(file);

    if let Err(error) = replace_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to finalize {label} from {} to {}",
                temp_path.display(),
                path.display()
            )
        });
    }
    sync_parent_directory(path)?;
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn configure_temporary_permissions(
    file: &std::fs::File,
    existing_permissions: Option<std::fs::Permissions>,
    private: bool,
) -> std::io::Result<()> {
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt as _;
        return file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }

    #[cfg(not(unix))]
    let _ = private;

    if let Some(permissions) = existing_permissions {
        file.set_permissions(permissions)?;
    }
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn create_temporary_file(path: &Path, label: &str) -> Result<(PathBuf, std::fs::File)> {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("annotations.json"))
        .to_os_string();
    loop {
        let suffix = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = file_name.clone();
        temp_name.push(format!(".{}.{suffix}.tmp", std::process::id()));
        let temp_path = path.with_file_name(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create temporary {label} {}", temp_path.display())
                });
            }
        }
    }
}

#[cfg(all(not(windows), not(all(target_arch = "wasm32", target_os = "unknown"))))]
fn replace_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return std::fs::rename(temp_path, path);
    }
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let replaced = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replacement = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are owned, NUL-terminated UTF-16 buffers that remain alive for the call;
    // the optional backup and reserved pointers are intentionally null.
    let replaced_ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced_ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn normalize_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current working directory")?
            .join(path)
    };

    match std::fs::canonicalize(&absolute) {
        Ok(canonical) => Ok(canonical),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut candidate = absolute.as_path();
            let mut missing = Vec::<OsString>::new();
            loop {
                match std::fs::canonicalize(candidate) {
                    Ok(mut canonical) => {
                        for component in missing.iter().rev() {
                            canonical.push(component);
                        }
                        return Ok(canonical);
                    }
                    Err(parent_error) if parent_error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(parent_error) => {
                        return Err(parent_error).with_context(|| {
                            format!("failed to resolve PDF ancestor {}", candidate.display())
                        });
                    }
                }

                let file_name = candidate.file_name().ok_or_else(|| {
                    anyhow!(
                        "failed to find an existing ancestor for PDF path {}",
                        absolute.display()
                    )
                })?;
                missing.push(file_name.to_os_string());
                candidate = candidate.parent().ok_or_else(|| {
                    anyhow!(
                        "failed to find an existing ancestor for PDF path {}",
                        absolute.display()
                    )
                })?;
            }
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to resolve PDF path {}", absolute.display()))
        }
    }
}

#[cfg(all(unix, not(all(target_arch = "wasm32", target_os = "unknown"))))]
fn sync_parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("failed to sync PDF output directory {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(all(not(unix), not(all(target_arch = "wasm32", target_os = "unknown"))))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(all(test, not(all(target_arch = "wasm32", target_os = "unknown"))))]
mod tests {
    use std::path::{Path, PathBuf};

    use futures::executor::block_on;
    use tempfile::tempdir;

    use crate::annotation::{
        Annotation, AnnotationId, PageAnnotation, PdfColor, PdfPoint, PdfRect,
    };
    use crate::page::PdfLinkDestination;

    use super::*;

    #[test]
    fn opens_extracts_text_and_generates_schematic_preview() {
        let pdf = make_test_pdf(
            &[("Hello PDF\nSecond line", (200.0, 240.0))],
            &[("Title", "Fixture Title"), ("Author", "Kael")],
        );
        let document = block_on(PdfDocument::open_from_memory(&pdf)).unwrap();

        assert_eq!(document.page_count(), 1);
        assert_eq!(document.metadata().title.as_deref(), Some("Fixture Title"));
        let page = document.page(0).unwrap();
        assert!(page.text().unwrap().contains("Hello PDF"));
        assert_eq!(page.search("second").unwrap().len(), 1);

        let rendered = block_on(page.schematic_preview(1.0)).unwrap();
        assert_eq!(rendered.width(), 200);
        assert_eq!(rendered.height(), 240);
        assert_eq!(rendered.pixels().len(), 200 * 240 * 4);
    }

    #[test]
    fn persists_annotations_in_sidecar_files() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        std::fs::write(
            &path,
            make_test_pdf(&[("Annotate me", (240.0, 320.0))], &[]),
        )
        .unwrap();

        let document = block_on(PdfDocument::open(&path)).unwrap();
        let page = document.page(0).unwrap();
        page.add_annotation(Annotation::Highlight {
            rects: vec![PdfRect::new(24.0, 200.0, 80.0, 16.0)],
            color: PdfColor::rgba(255, 240, 120, 160),
        })
        .unwrap();
        page.add_annotation(Annotation::Note {
            position: PdfPoint::new(64.0, 64.0),
            text: "Remember this".to_string(),
        })
        .unwrap();
        assert!(document.has_unsaved_annotations());
        block_on(document.save_annotations()).unwrap();
        assert!(!document.has_unsaved_annotations());

        let reopened = block_on(PdfDocument::open(&path)).unwrap();
        let annotations = reopened.page(0).unwrap().annotations();
        assert_eq!(annotations.len(), 2);
        let sidecar = path.with_file_name("notes.pdf.annotations.json");
        assert!(sidecar.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(sidecar).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn rejects_invalid_annotations_and_exhausted_ids_without_mutating() {
        let pdf = make_test_pdf(&[("Page", (200.0, 240.0))], &[]);
        let document = block_on(PdfDocument::open_from_memory(&pdf)).unwrap();
        let page = document.page(0).unwrap();

        assert!(
            page.add_annotation(Annotation::Note {
                position: PdfPoint::new(f32::NAN, 0.0),
                text: "bad".into(),
            })
            .is_err()
        );
        document.inner.lock().next_annotation_id = u64::MAX;
        assert!(
            page.add_annotation(Annotation::Note {
                position: PdfPoint::new(1.0, 1.0),
                text: "valid".into(),
            })
            .is_err()
        );
        assert!(page.annotations().is_empty());

        {
            let mut state = document.inner.lock();
            state.next_annotation_id = 1;
            state.annotation_model_bytes = MAX_ANNOTATION_MODEL_BYTES;
        }
        assert!(
            page.add_annotation(Annotation::Note {
                position: PdfPoint::new(1.0, 1.0),
                text: "bounded".into(),
            })
            .is_err()
        );
        assert!(page.annotations().is_empty());
    }

    #[test]
    fn ignores_invalid_sidecars_and_reports_a_warning() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        std::fs::write(&path, make_test_pdf(&[("Page", (200.0, 240.0))], &[])).unwrap();
        let annotation = PageAnnotation {
            id: AnnotationId(1),
            kind: Annotation::Note {
                position: PdfPoint::new(1.0, 1.0),
                text: "note".into(),
            },
        };
        let sidecar_path = annotations_sidecar_path(&path);
        let mut pages = BTreeMap::new();
        pages.insert(0, vec![annotation.clone(), annotation]);
        std::fs::write(
            &sidecar_path,
            serde_json::to_vec(&AnnotationSidecar {
                document_sha256: digest_file(&path).unwrap(),
                pages,
            })
            .unwrap(),
        )
        .unwrap();
        let document = block_on(PdfDocument::open(&path)).unwrap();
        assert!(document.annotation_load_warning().is_some());
        assert!(document.page(0).unwrap().annotations().is_empty());

        let mut pages = BTreeMap::new();
        pages.insert(
            1,
            vec![PageAnnotation {
                id: AnnotationId(2),
                kind: Annotation::Note {
                    position: PdfPoint::new(1.0, 1.0),
                    text: "note".into(),
                },
            }],
        );
        std::fs::write(
            sidecar_path,
            serde_json::to_vec(&AnnotationSidecar {
                document_sha256: digest_file(&path).unwrap(),
                pages,
            })
            .unwrap(),
        )
        .unwrap();
        let document = block_on(PdfDocument::open(&path)).unwrap();
        assert!(document.annotation_load_warning().is_some());
        assert!(document.page(0).unwrap().annotations().is_empty());
    }

    #[test]
    fn ignores_sidecars_from_different_document_contents() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        std::fs::write(&path, make_test_pdf(&[("First", (200.0, 240.0))], &[])).unwrap();
        let document = block_on(PdfDocument::open(&path)).unwrap();
        document
            .page(0)
            .unwrap()
            .add_annotation(Annotation::Note {
                position: PdfPoint::new(1.0, 1.0),
                text: "note".into(),
            })
            .unwrap();
        block_on(document.save(&path)).unwrap();

        std::fs::write(
            &path,
            make_test_pdf(&[("Replacement", (200.0, 240.0))], &[]),
        )
        .unwrap();

        let reopened = block_on(PdfDocument::open(&path)).unwrap();
        assert!(
            reopened
                .annotation_load_warning()
                .unwrap()
                .contains("does not match")
        );
        assert!(reopened.page(0).unwrap().annotations().is_empty());
    }

    #[test]
    fn discovers_uri_links_and_nested_outline_entries() {
        let pdf = make_test_pdf(
            &[("First", (200.0, 240.0)), ("Second", (200.0, 240.0))],
            &[],
        );
        let mut loaded = LoDocument::load_mem(&pdf).unwrap();
        let page_ids = loaded.get_pages().values().copied().collect::<Vec<_>>();

        let link_id = loaded.new_object_id();
        let mut action = lopdf::Dictionary::new();
        action.set("S", Object::Name(b"URI".to_vec()));
        action.set(
            "URI",
            Object::String(
                b"https://example.com".to_vec(),
                lopdf::StringFormat::Literal,
            ),
        );
        let mut link = lopdf::Dictionary::new();
        link.set("Subtype", Object::Name(b"Link".to_vec()));
        link.set(
            "Rect",
            Object::Array(vec![10.into(), 20.into(), 80.into(), 40.into()]),
        );
        link.set("A", Object::Dictionary(action));
        loaded.objects.insert(link_id, Object::Dictionary(link));
        loaded
            .get_object_mut(page_ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", Object::Array(vec![Object::Reference(link_id)]));

        let outlines_id = loaded.new_object_id();
        let first_id = loaded.new_object_id();
        let child_id = loaded.new_object_id();
        let mut outlines = lopdf::Dictionary::new();
        outlines.set("First", Object::Reference(first_id));
        let mut first = lopdf::Dictionary::new();
        first.set(
            "Title",
            Object::String(b"Chapter".to_vec(), lopdf::StringFormat::Literal),
        );
        first.set(
            "Dest",
            Object::Array(vec![
                Object::Reference(page_ids[0]),
                Object::Name(b"Fit".to_vec()),
            ]),
        );
        first.set("First", Object::Reference(child_id));
        let mut child = lopdf::Dictionary::new();
        child.set(
            "Title",
            Object::String(b"Section".to_vec(), lopdf::StringFormat::Literal),
        );
        child.set(
            "Dest",
            Object::Array(vec![
                Object::Reference(page_ids[1]),
                Object::Name(b"Fit".to_vec()),
            ]),
        );
        loaded
            .objects
            .insert(outlines_id, Object::Dictionary(outlines));
        loaded.objects.insert(first_id, Object::Dictionary(first));
        loaded.objects.insert(child_id, Object::Dictionary(child));
        let root_id = loaded.trailer.get(b"Root").unwrap().as_reference().unwrap();
        loaded
            .get_object_mut(root_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Outlines", Object::Reference(outlines_id));

        let document = PdfDocument::from_loaded(loaded, None, None).unwrap();
        let links = document.page(0).unwrap().links().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].destination,
            PdfLinkDestination::Uri("https://example.com".into())
        );
        let outline = document.outline();
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].title, "Chapter");
        assert_eq!(outline[0].page_index, Some(0));
        assert_eq!(outline[0].children[0].page_index, Some(1));
    }

    #[test]
    fn cyclic_page_parent_hierarchy_is_rejected() {
        let pdf = make_test_pdf(&[("Page", (200.0, 240.0))], &[]);
        let mut loaded = LoDocument::load_mem(&pdf).unwrap();
        let page_id = *loaded.get_pages().values().next().unwrap();
        let page = loaded
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap();
        page.remove(b"MediaBox");
        page.set("Parent", Object::Reference(page_id));

        assert!(page_size_for_object(&loaded, page_id).is_err());
    }

    #[test]
    fn size_checks_and_atomic_writes_reject_invalid_targets() {
        assert!(ensure_size(MAX_PDF_BYTES, MAX_PDF_BYTES, "PDF").is_ok());
        assert!(ensure_size(MAX_PDF_BYTES + 1, MAX_PDF_BYTES, "PDF").is_err());
        let directory = tempdir().unwrap();
        let target = directory.path().join("directory-target");
        std::fs::create_dir(&target).unwrap();
        assert!(write_bytes_atomically(&target, b"data").is_err());
        assert!(target.is_dir());

        let oversized = directory.path().join("oversized");
        assert!(
            write_file_atomically(&oversized, "bounded output", false, 4, |writer| {
                writer.write_all(b"12345").map_err(Into::into)
            })
            .is_err()
        );
        assert!(!oversized.exists());
        std::fs::write(&oversized, b"12345").unwrap();
        assert!(read_regular_file_bounded(&oversized, 4, "bounded input").is_err());

        let document = LoDocument::new();
        assert_eq!(
            bounded_pdf_string(&document, &lopdf::text_string("Résumé"), 64).as_deref(),
            Some("Résumé")
        );
        assert!(bounded_pdf_string(&document, &lopdf::text_string("Résumé"), 3).is_none());
    }

    #[test]
    fn link_destinations_are_bounded() {
        let document = LoDocument::new();
        let page_indices = BTreeMap::new();
        let mut destination = Object::Name(b"chapter".to_vec());
        for _ in 0..130 {
            destination = Object::Array(vec![destination]);
        }
        assert!(parse_destination(&document, &destination, &page_indices).is_none());
        assert!(
            bounded_destination_text(
                &document,
                &Object::String(
                    b"https://example.com/\nunsafe".to_vec(),
                    lopdf::StringFormat::Literal,
                )
            )
            .is_none()
        );
    }

    #[test]
    fn cache_entry_counts_are_bounded() {
        let mut text_cache = PageTextCache::default();
        for index in 0..=MAX_TEXT_CACHE_ENTRIES {
            text_cache.insert(index, Arc::<str>::from("x"));
        }
        assert_eq!(text_cache.entries.len(), MAX_TEXT_CACHE_ENTRIES);
        assert!(text_cache.get(0).is_none());

        let mut preview_cache = PagePreviewCache::default();
        for index in 0..=MAX_PREVIEW_CACHE_ENTRIES {
            preview_cache.insert(
                (index, 1.0f32.to_bits(), 0),
                PagePreview::new(1, 1, Arc::<[u8]>::from([0, 0, 0, 0])).unwrap(),
            );
        }
        assert_eq!(preview_cache.entries.len(), MAX_PREVIEW_CACHE_ENTRIES);
        assert!(preview_cache.get((0, 1.0f32.to_bits(), 0)).is_none());
    }

    #[test]
    fn sidecar_failure_reports_that_the_pdf_was_saved() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        let pdf = make_test_pdf(&[("Page", (200.0, 240.0))], &[]);
        let document = block_on(PdfDocument::open_from_memory(&pdf)).unwrap();
        document
            .page(0)
            .unwrap()
            .add_annotation(Annotation::Note {
                position: PdfPoint::new(1.0, 1.0),
                text: "note".into(),
            })
            .unwrap();
        std::fs::create_dir(annotations_sidecar_path(&path)).unwrap();

        let error = block_on(document.save(&path)).unwrap_err();

        assert!(error.to_string().contains("PDF was saved"));
        assert!(document.has_unsaved_annotations());
        assert!(block_on(PdfDocument::open(&path)).is_ok());

        std::fs::remove_dir(annotations_sidecar_path(&path)).unwrap();
        block_on(document.save_annotations()).unwrap();
        assert!(!document.has_unsaved_annotations());
    }

    #[test]
    fn sidecar_only_save_rejects_an_externally_changed_pdf() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        std::fs::write(&path, make_test_pdf(&[("Original", (200.0, 240.0))], &[])).unwrap();
        let document = block_on(PdfDocument::open(&path)).unwrap();
        document
            .page(0)
            .unwrap()
            .add_annotation(Annotation::Note {
                position: PdfPoint::new(1.0, 1.0),
                text: "note".into(),
            })
            .unwrap();
        std::fs::write(
            &path,
            make_test_pdf(&[("External change", (200.0, 240.0))], &[]),
        )
        .unwrap();

        let error = block_on(document.save_annotations()).unwrap_err();

        assert!(error.to_string().contains("changed on disk"));
        assert!(document.has_unsaved_annotations());
        assert!(!annotations_sidecar_path(&path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn saves_through_existing_symbolic_links_without_replacing_them() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target = directory.path().join("target.pdf");
        let link = directory.path().join("link.pdf");
        std::fs::write(&target, make_test_pdf(&[("Old", (200.0, 240.0))], &[])).unwrap();
        symlink(&target, &link).unwrap();
        let replacement = make_test_pdf(&[("Replacement", (300.0, 400.0))], &[]);
        let document = block_on(PdfDocument::open_from_memory(&replacement)).unwrap();

        block_on(document.save(&link)).unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let reopened = block_on(PdfDocument::open(&target)).unwrap();
        assert_eq!(reopened.page(0).unwrap().size().unwrap().width, 300.0);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_sidecars_are_ignored() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let path = directory.path().join("notes.pdf");
        let outside = directory.path().join("outside.json");
        std::fs::write(&path, make_test_pdf(&[("Page", (200.0, 240.0))], &[])).unwrap();
        std::fs::write(&outside, b"{}").unwrap();
        symlink(&outside, annotations_sidecar_path(&path)).unwrap();

        let document = block_on(PdfDocument::open(&path)).unwrap();

        assert!(
            document
                .annotation_load_warning()
                .unwrap()
                .contains("not a regular file")
        );
        assert!(document.page(0).unwrap().annotations().is_empty());
        assert!(block_on(document.save_annotations()).is_err());
        assert!(
            std::fs::symlink_metadata(annotations_sidecar_path(&path))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&outside).unwrap(), b"{}");
    }

    #[test]
    fn missing_pdf_parents_are_normalized_from_the_nearest_existing_ancestor() {
        let directory = tempdir().unwrap();
        let existing = directory.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        let requested = existing.join("missing").join("nested").join("note.pdf");

        let normalized = normalize_path(&requested).unwrap();

        assert_eq!(
            normalized,
            std::fs::canonicalize(existing)
                .unwrap()
                .join("missing")
                .join("nested")
                .join("note.pdf")
        );
    }

    #[cfg(unix)]
    #[test]
    fn pdf_path_normalization_preserves_symlink_parent_semantics() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target_parent = directory.path().join("outside");
        let target = target_parent.join("target");
        let link = directory.path().join("link");
        std::fs::create_dir_all(&target).unwrap();
        symlink(&target, &link).unwrap();

        let normalized = normalize_path(&link.join("..").join("note.pdf")).unwrap();

        assert_eq!(
            normalized,
            std::fs::canonicalize(target_parent)
                .unwrap()
                .join("note.pdf")
        );
    }

    fn make_test_pdf(
        pages: &[(impl AsRef<str>, (f32, f32))],
        metadata: &[(&str, &str)],
    ) -> Vec<u8> {
        let font_id = 3u32;
        let mut next_id = 4u32;
        let mut page_ids = Vec::new();
        let mut content_ids = Vec::new();
        for _ in pages {
            page_ids.push(next_id);
            content_ids.push(next_id + 1);
            next_id += 2;
        }
        let info_id = (!metadata.is_empty()).then_some(next_id);

        let mut objects = Vec::new();
        objects.push((1, String::from("<< /Type /Catalog /Pages 2 0 R >>")));
        objects.push((
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                page_ids
                    .iter()
                    .map(|id| format!("{id} 0 R"))
                    .collect::<Vec<_>>()
                    .join(" "),
                page_ids.len()
            ),
        ));
        objects.push((
            font_id,
            String::from("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ));

        for (index, (text, (width, height))) in pages.iter().enumerate() {
            let stream = page_stream(text.as_ref(), *height);
            let page_id = page_ids[index];
            let content_id = content_ids[index];
            objects.push((
                page_id,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 {} 0 R >> >> /Contents {} 0 R >>",
                    format_number(*width),
                    format_number(*height),
                    font_id,
                    content_id
                ),
            ));
            objects.push((
                content_id,
                format!(
                    "<< /Length {} >>\nstream\n{}\nendstream",
                    stream.len(),
                    stream
                ),
            ));
        }

        if let Some(info_id) = info_id {
            let mut body = String::from("<<");
            for (key, value) in metadata {
                body.push(' ');
                body.push('/');
                body.push_str(key);
                body.push(' ');
                body.push('(');
                body.push_str(&escape_pdf_text(value));
                body.push(')');
            }
            body.push_str(" >>");
            objects.push((info_id, body));
        }

        assemble_pdf(&objects, info_id)
    }

    fn assemble_pdf(objects: &[(u32, String)], info_id: Option<u32>) -> Vec<u8> {
        let mut pdf = Vec::from(&b"%PDF-1.4\n%\xFF\xFF\xFF\xFF\n"[..]);
        let max_id = objects.iter().map(|(id, _)| *id).max().unwrap_or(0);
        let mut offsets = vec![0usize; max_id as usize + 1];

        for (id, body) in objects {
            offsets[*id as usize] = pdf.len();
            pdf.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
        }

        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        let mut trailer = format!("trailer\n<< /Size {} /Root 1 0 R", max_id + 1);
        if let Some(info_id) = info_id {
            trailer.push_str(&format!(" /Info {info_id} 0 R"));
        }
        trailer.push_str(" >>\n");
        pdf.extend_from_slice(trailer.as_bytes());
        pdf.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF\n").as_bytes());
        pdf
    }

    fn page_stream(text: &str, page_height: f32) -> String {
        let mut stream = String::from("BT\n/F1 12 Tf\n");
        let mut first = true;
        for line in text.lines() {
            if first {
                stream.push_str(&format!("72 {} Td\n", format_number(page_height - 72.0)));
                first = false;
            } else {
                stream.push_str("0 -16 Td\n");
            }
            stream.push('(');
            stream.push_str(&escape_pdf_text(line));
            stream.push_str(") Tj\n");
        }
        stream.push_str("ET");
        stream
    }

    fn format_number(value: f32) -> String {
        if value.fract() == 0.0 {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        }
    }

    fn escape_pdf_text(text: &str) -> String {
        text.replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }

    #[allow(dead_code)]
    fn sidecar_path_for(path: &Path) -> PathBuf {
        path.with_file_name(format!(
            "{}.annotations.json",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("document.pdf")
        ))
    }
}
