//! PDF document loading and persistence.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result, anyhow};
use lopdf::{Document as LoDocument, Object, ObjectId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    annotation::{Annotation, AnnotationId, PageAnnotation},
    page::{PdfLink, PdfPage, PdfPageSize},
    renderer::{RenderedPage, render_page_preview},
    text::{TextMatch, extract_page_text, search_text},
};

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
}

#[derive(Debug, Clone)]
struct PageDescriptor {
    page_number: u32,
    size: PdfPageSize,
}

struct DocumentState {
    document: LoDocument,
    source_path: Option<PathBuf>,
    pages: Vec<PageDescriptor>,
    metadata: PdfMetadata,
    outline: Vec<OutlineItem>,
    page_text_cache: BTreeMap<usize, Arc<str>>,
    rendered_page_cache: BTreeMap<(usize, u32, u64), RenderedPage>,
    annotations: BTreeMap<usize, Arc<[PageAnnotation]>>,
    annotation_generation: u64,
    next_annotation_id: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AnnotationSidecar {
    pages: BTreeMap<usize, Vec<PageAnnotation>>,
}

impl PdfDocument {
    /// Opens a PDF document from disk.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        smol::unblock(move || {
            let document = LoDocument::load(&path)
                .with_context(|| format!("failed to open PDF document {}", path.display()))?;
            Self::from_loaded(document, Some(path))
        })
        .await
    }

    /// Opens a PDF document from in-memory bytes.
    pub async fn open_from_memory(data: &[u8]) -> Result<Self> {
        let data = data.to_vec();
        smol::unblock(move || {
            let document =
                LoDocument::load_mem(&data).context("failed to open PDF document from memory")?;
            Self::from_loaded(document, None)
        })
        .await
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

    /// Saves the current document and sidecar annotations to disk.
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let inner = self.inner.clone();

        smol::unblock(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create PDF output directory {}", parent.display())
                })?;
            }

            let mut state = inner.lock();
            state
                .document
                .save(&path)
                .with_context(|| format!("failed to save PDF document {}", path.display()))?;
            persist_annotations(&path, &state.annotations)?;
            state.source_path = Some(path);
            Ok(())
        })
        .await
    }

    pub(crate) fn page_size(&self, page_index: usize) -> Result<PdfPageSize> {
        Ok(self.page_descriptor(page_index)?.size)
    }

    pub(crate) fn page_text(&self, page_index: usize) -> Result<String> {
        Ok(self.page_text_shared(page_index)?.to_string())
    }

    fn page_text_shared(&self, page_index: usize) -> Result<Arc<str>> {
        let mut state = self.inner.lock();
        if let Some(text) = state.page_text_cache.get(&page_index) {
            return Ok(text.clone());
        }

        let page_number = state
            .pages
            .get(page_index)
            .ok_or_else(|| anyhow!("page index {page_index} out of range"))?
            .page_number;
        let text = Arc::<str>::from(extract_page_text(&state.document, page_number)?);
        state.page_text_cache.insert(page_index, text.clone());
        Ok(text)
    }

    pub(crate) fn search_page_text(&self, page_index: usize, query: &str) -> Vec<TextMatch> {
        self.page_text_shared(page_index)
            .map(|text| search_text(page_index, text.as_ref(), query))
            .unwrap_or_default()
    }

    pub(crate) fn render_page(&self, page_index: usize, scale: f32) -> Result<RenderedPage> {
        let scale = scale.clamp(0.25, 4.0);
        let (page_size, annotation_generation, cached_page) = {
            let state = self.inner.lock();
            let page_size = state
                .pages
                .get(page_index)
                .ok_or_else(|| anyhow!("page index {page_index} out of range"))?
                .size;
            let annotation_generation = state.annotation_generation;
            let cached_page = state
                .rendered_page_cache
                .get(&(page_index, scale.to_bits(), annotation_generation))
                .cloned();
            (page_size, annotation_generation, cached_page)
        };

        if let Some(cached_page) = cached_page {
            return Ok(cached_page);
        }

        let text = self
            .page_text_shared(page_index)
            .unwrap_or_else(|_| Arc::<str>::from(""));
        let annotations = self.page_annotations_shared(page_index);
        let rendered = render_page_preview(page_size, text.as_ref(), &annotations, scale)?;

        let mut state = self.inner.lock();
        if state.annotation_generation == annotation_generation {
            state.rendered_page_cache.insert(
                (page_index, scale.to_bits(), annotation_generation),
                rendered.clone(),
            );
        }

        Ok(rendered)
    }

    pub(crate) fn page_links(&self, _page_index: usize) -> Vec<PdfLink> {
        Vec::new()
    }

    pub(crate) fn page_annotations(&self, page_index: usize) -> Vec<PageAnnotation> {
        self.page_annotations_shared(page_index)
            .iter()
            .cloned()
            .collect()
    }

    fn page_annotations_shared(&self, page_index: usize) -> Arc<[PageAnnotation]> {
        self.inner
            .lock()
            .annotations
            .get(&page_index)
            .cloned()
            .unwrap_or_else(|| Arc::<[PageAnnotation]>::from([]))
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
        let entry = PageAnnotation {
            id: AnnotationId(state.next_annotation_id),
            kind: annotation,
        };
        state.next_annotation_id += 1;
        let annotations = state
            .annotations
            .entry(page_index)
            .or_insert_with(|| Arc::<[PageAnnotation]>::from([]));
        let mut next_annotations = annotations.as_ref().to_vec();
        next_annotations.push(entry);
        *annotations = Arc::from(next_annotations);
        state.annotation_generation += 1;
        state.rendered_page_cache.clear();
        Ok(())
    }

    pub(crate) fn remove_page_annotation(&self, page_index: usize, id: AnnotationId) -> Result<()> {
        let mut state = self.inner.lock();
        let annotations = state
            .annotations
            .get_mut(&page_index)
            .ok_or_else(|| anyhow!("page index {page_index} has no annotations"))?;
        let mut next_annotations = annotations.as_ref().to_vec();
        let previous_len = next_annotations.len();
        next_annotations.retain(|annotation| annotation.id != id);
        if next_annotations.len() == previous_len {
            return Err(anyhow!(
                "annotation {} not found on page {}",
                id.0,
                page_index
            ));
        }
        if next_annotations.is_empty() {
            state.annotations.remove(&page_index);
        } else {
            *annotations = Arc::from(next_annotations);
        }
        state.annotation_generation += 1;
        state.rendered_page_cache.clear();
        Ok(())
    }

    fn from_loaded(document: LoDocument, source_path: Option<PathBuf>) -> Result<Self> {
        let pages = collect_pages(&document)?;
        let metadata = extract_metadata(&document);
        let annotations = if let Some(path) = source_path.as_ref() {
            load_annotations(path)?
        } else {
            BTreeMap::new()
        };
        let next_annotation_id = next_annotation_id(&annotations);

        Ok(Self {
            inner: Arc::new(Mutex::new(DocumentState {
                document,
                source_path,
                pages,
                metadata,
                outline: Vec::new(),
                page_text_cache: BTreeMap::new(),
                rendered_page_cache: BTreeMap::new(),
                annotations,
                annotation_generation: 0,
                next_annotation_id,
            })),
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
    let mut pages = Vec::new();
    for (page_number, object_id) in document.get_pages() {
        pages.push(PageDescriptor {
            page_number,
            size: page_size_for_object(document, object_id)?,
        });
    }
    Ok(pages)
}

fn page_size_for_object(document: &LoDocument, object_id: ObjectId) -> Result<PdfPageSize> {
    if let Some(media_box) = media_box_for_object(document, object_id)? {
        let numbers = media_box
            .iter()
            .map(|value| object_number(value))
            .collect::<Result<Vec<_>>>()?;
        if numbers.len() == 4 {
            return Ok(PdfPageSize::new(
                (numbers[2] - numbers[0]).abs().max(1.0),
                (numbers[3] - numbers[1]).abs().max(1.0),
            ));
        }
    }

    Ok(PdfPageSize::new(612.0, 792.0))
}

fn media_box_for_object(document: &LoDocument, object_id: ObjectId) -> Result<Option<Vec<Object>>> {
    let dictionary = document.get_object(object_id)?.as_dict()?;
    if let Ok(media_box) = dictionary.get(b"MediaBox") {
        return Ok(Some(resolve_array(document, media_box)?));
    }
    if let Ok(parent) = dictionary.get(b"Parent") {
        if let Ok(parent_id) = parent.as_reference() {
            return media_box_for_object(document, parent_id);
        }
    }
    Ok(None)
}

fn resolve_array(document: &LoDocument, object: &Object) -> Result<Vec<Object>> {
    match object {
        Object::Array(values) => Ok(values.clone()),
        Object::Reference(object_id) => resolve_array(document, document.get_object(*object_id)?),
        _ => Err(anyhow!("expected PDF array object")),
    }
}

fn object_number(object: &Object) -> Result<f32> {
    match object {
        Object::Integer(value) => Ok(*value as f32),
        Object::Real(value) => Ok(*value),
        _ => Err(anyhow!("expected numeric PDF object")),
    }
}

fn extract_metadata(document: &LoDocument) -> PdfMetadata {
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

    metadata.title = dictionary.get(b"Title").ok().map(pdf_string);
    metadata.author = dictionary.get(b"Author").ok().map(pdf_string);
    metadata.subject = dictionary.get(b"Subject").ok().map(pdf_string);
    metadata.creator = dictionary.get(b"Creator").ok().map(pdf_string);
    metadata.producer = dictionary.get(b"Producer").ok().map(pdf_string);
    metadata.keywords = dictionary
        .get(b"Keywords")
        .ok()
        .map(pdf_string)
        .map(|keywords| {
            keywords
                .split(|character| matches!(character, ',' | ';'))
                .map(str::trim)
                .filter(|keyword| !keyword.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    metadata
}

fn pdf_string(object: &Object) -> String {
    match object {
        Object::String(bytes, _) => String::from_utf8_lossy(bytes).into_owned(),
        Object::Name(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => format!("{object:?}"),
    }
}

fn load_annotations(path: &Path) -> Result<BTreeMap<usize, Arc<[PageAnnotation]>>> {
    let sidecar_path = annotations_sidecar_path(path);
    if !sidecar_path.exists() {
        return Ok(BTreeMap::new());
    }

    let json = std::fs::read(&sidecar_path).with_context(|| {
        format!(
            "failed to read annotation sidecar {}",
            sidecar_path.display()
        )
    })?;
    let sidecar: AnnotationSidecar =
        serde_json::from_slice(&json).context("failed to deserialize annotation sidecar")?;
    Ok(sidecar
        .pages
        .into_iter()
        .map(|(page_index, annotations)| (page_index, Arc::<[PageAnnotation]>::from(annotations)))
        .collect())
}

fn persist_annotations(
    path: &Path,
    annotations: &BTreeMap<usize, Arc<[PageAnnotation]>>,
) -> Result<()> {
    let sidecar_path = annotations_sidecar_path(path);
    if annotations.is_empty() {
        match std::fs::remove_file(&sidecar_path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to remove annotation sidecar {}",
                        sidecar_path.display()
                    )
                });
            }
        }
    }

    let json = serde_json::to_vec(&AnnotationSidecar {
        pages: annotations
            .iter()
            .map(|(page_index, annotations)| (*page_index, annotations.as_ref().to_vec()))
            .collect(),
    })
    .context("failed to serialize annotation sidecar")?;
    write_bytes_atomically(&sidecar_path, &json)
}

fn annotations_sidecar_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.pdf");
    path.with_file_name(format!("{file_name}.annotations.json"))
}

fn next_annotation_id(annotations: &BTreeMap<usize, Arc<[PageAnnotation]>>) -> u64 {
    annotations
        .values()
        .flat_map(|entries| entries.iter().map(|entry| entry.id.0))
        .max()
        .map(|value| value + 1)
        .unwrap_or(1)
}

fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create annotation sidecar directory {}",
                parent.display()
            )
        })?;
    }

    let temp_path = temporary_path(path);
    std::fs::write(&temp_path, bytes).with_context(|| {
        format!(
            "failed to write annotation temp file {}",
            temp_path.display()
        )
    })?;
    std::fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to finalize annotation sidecar from {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("annotations.json");
    let unique_suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    path.with_file_name(format!(
        "{file_name}.{}.{}.tmp",
        std::process::id(),
        unique_suffix
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use futures::executor::block_on;
    use tempfile::tempdir;

    use crate::annotation::{Annotation, PdfColor, PdfPoint, PdfRect};

    use super::PdfDocument;

    #[test]
    fn opens_extracts_text_and_renders_generated_pdf() {
        let pdf = make_test_pdf(
            &[("Hello PDF\nSecond line", (200.0, 240.0))],
            &[("Title", "Fixture Title"), ("Author", "Kael")],
        );
        let document = block_on(PdfDocument::open_from_memory(&pdf)).unwrap();

        assert_eq!(document.page_count(), 1);
        assert_eq!(document.metadata().title.as_deref(), Some("Fixture Title"));
        let page = document.page(0).unwrap();
        assert!(page.text().contains("Hello PDF"));
        assert_eq!(page.search("second").len(), 1);

        let rendered = block_on(page.render(1.0)).unwrap();
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
        block_on(document.save(&path)).unwrap();

        let reopened = block_on(PdfDocument::open(&path)).unwrap();
        let annotations = reopened.page(0).unwrap().annotations();
        assert_eq!(annotations.len(), 2);
        let sidecar = path.with_file_name("notes.pdf.annotations.json");
        assert!(sidecar.exists());
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
