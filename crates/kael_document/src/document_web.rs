//! Browser document controller with byte import/export and IndexedDB persistence.

use std::{
    collections::{BTreeMap, VecDeque},
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use kael_storage::BlobStore;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    AutosaveConfig, DocumentExport, DocumentPlatformError, DocumentVersion, FileType,
    RecentDocument, StoredDocument, Subscription,
    file_type::{default_file_type_index, file_type_index_for_path},
};

type ChangeListener<T> = Arc<dyn Fn(&T) + Send + Sync + 'static>;
type DirtyListener = Arc<dyn Fn(bool) + Send + Sync + 'static>;
type Snapshot<T> = Arc<T>;

const MAX_DOCUMENT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENVELOPE_METADATA_BYTES: usize = 1024 * 1024;
const MAX_DOCUMENT_ID_BYTES: usize = 512;
const DOCUMENT_MAGIC: &[u8] = b"KAEL-WEB-DOCUMENT\0";
const DOCUMENT_VERSION: u8 = 1;
const AUTOSAVE_MAGIC: &[u8] = b"KAEL-WEB-AUTOSAVE\0";
const AUTOSAVE_VERSION: u8 = 1;
const DOCUMENT_KEY_PREFIX: &str = "document:";
const AUTOSAVE_KEY_PREFIX: &str = "autosave:";
const VERSION_KEY_PREFIX: &str = "version:";

/// A document format supported by the controller.
///
/// This is the same byte-oriented contract used by native Kael. A `Document` implementation is
/// responsible for its own formats; `kael_document` does not itself parse DOCX, XLSX, or PPTX.
pub trait Document: Send + Sync + 'static {
    /// The document content model.
    type Content: Clone + PartialEq + Send + Sync;

    /// Returns the supported file types for this document.
    fn file_types() -> &'static [FileType];

    /// Creates a new untitled document.
    fn new_untitled() -> Self::Content;

    /// Parses document bytes into content.
    fn read(data: &[u8], file_type: &FileType) -> Result<Self::Content>;

    /// Serializes content into document bytes.
    fn write(content: &Self::Content, file_type: &FileType) -> Result<Vec<u8>>;
}

/// A controller that manages browser documents of a specific type.
pub struct DocumentController<D: Document> {
    inner: Arc<ControllerState<D>>,
}

/// A mutable browser document handle managed by a controller.
pub struct DocumentHandle<D: Document> {
    controller: Arc<ControllerState<D>>,
    state: Arc<Mutex<DocumentState<D>>>,
    operation_lock: Arc<async_lock::Mutex<()>>,
}

impl<D: Document> Clone for DocumentController<D> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<D: Document> Clone for DocumentHandle<D> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
            state: self.state.clone(),
            operation_lock: self.operation_lock.clone(),
        }
    }
}

struct ControllerState<D: Document> {
    app_id: String,
    store: Option<BlobStore>,
    autosave_config: AutosaveConfig,
    max_history_depth: usize,
    max_versions: usize,
    marker: PhantomData<fn() -> D>,
}

impl<D: Document> Clone for ControllerState<D> {
    fn clone(&self) -> Self {
        Self {
            app_id: self.app_id.clone(),
            store: self.store.clone(),
            autosave_config: self.autosave_config.clone(),
            max_history_depth: self.max_history_depth,
            max_versions: self.max_versions,
            marker: PhantomData,
        }
    }
}

struct DocumentState<D: Document> {
    name: String,
    content: Snapshot<D::Content>,
    last_saved_snapshot: Option<Snapshot<D::Content>>,
    last_saved_digest: Option<String>,
    stored_id: Option<String>,
    file_type_index: usize,
    dirty: bool,
    versions: Vec<DocumentVersion>,
    undo_stack: VecDeque<Snapshot<D::Content>>,
    redo_stack: VecDeque<Snapshot<D::Content>>,
    next_listener_id: usize,
    change_listeners: BTreeMap<usize, ChangeListener<D::Content>>,
    dirty_listeners: BTreeMap<usize, DirtyListener>,
    autosave_revision: u64,
    last_autosave_error: Option<String>,
}

impl<D: Document> DocumentState<D> {
    fn allocate_listener_id(&mut self) -> usize {
        loop {
            let candidate = self.next_listener_id;
            self.next_listener_id = self.next_listener_id.checked_add(1).unwrap_or(0);
            if !self.change_listeners.contains_key(&candidate)
                && !self.dirty_listeners.contains_key(&candidate)
            {
                return candidate;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMetadata {
    id: String,
    name: String,
    file_type_index: usize,
    size_bytes: usize,
    modified_at_millis: u64,
    digest: String,
    versions: Vec<DocumentVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutosaveMetadata {
    baseline_digest: String,
    size_bytes: usize,
}

impl<D: Document> DocumentController<D> {
    /// Creates an in-memory browser controller.
    ///
    /// Byte import/export, editing, listeners, undo, and redo work immediately. Call
    /// [`Self::new_persistent`] when named documents, versions, and recovery snapshots must survive
    /// a reload.
    pub fn new(app_id: impl Into<String>) -> Result<Self> {
        Self::new_with_store(app_id.into(), None)
    }

    /// Opens the controller's origin-scoped IndexedDB document store.
    pub async fn new_persistent(app_id: impl Into<String>) -> Result<Self> {
        let app_id = app_id.into();
        let store = BlobStore::open(&app_id, "documents")
            .await
            .context("failed to open the browser document store")?;
        Self::new_with_store(app_id, Some(store))
    }

    /// Returns a typed error because a browser cannot root document metadata at a native path.
    pub fn new_in(_app_id: impl Into<String>, _storage_root: impl AsRef<Path>) -> Result<Self> {
        Err(DocumentPlatformError::NativePathUnavailable {
            operation: "DocumentController::new_in",
        }
        .into())
    }

    fn new_with_store(app_id: String, store: Option<BlobStore>) -> Result<Self> {
        validate_document_id(&app_id)?;
        Ok(Self {
            inner: Arc::new(ControllerState {
                app_id,
                store,
                autosave_config: AutosaveConfig::default(),
                max_history_depth: 100,
                max_versions: 20,
                marker: PhantomData,
            }),
        })
    }

    /// Returns a controller configured with a custom autosave strategy.
    ///
    /// Browser recovery always uses origin-scoped IndexedDB; the value remains useful to shared
    /// source code and native builds.
    pub fn with_autosave_config(mut self, autosave_config: AutosaveConfig) -> Self {
        Arc::make_mut(&mut self.inner).autosave_config = autosave_config;
        self
    }

    /// Returns a controller configured with a custom undo history depth.
    pub fn with_history_limit(mut self, max_history_depth: usize) -> Self {
        Arc::make_mut(&mut self.inner).max_history_depth = max_history_depth.max(1);
        self
    }

    /// Creates a new untitled document.
    pub fn new_document(&self) -> DocumentHandle<D> {
        static NEXT_UNTITLED_ID: AtomicU64 = AtomicU64::new(1);
        let untitled_id = NEXT_UNTITLED_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!("untitled-{untitled_id}");
        let file_type_index = default_file_type_index(D::file_types()).unwrap_or(0);
        let content = Arc::new(D::new_untitled());
        self.make_handle(
            name,
            content.clone(),
            Some(content),
            None,
            file_type_index,
            false,
            Vec::new(),
        )
    }

    /// Parses file-picker or drag-and-drop bytes without relying on a native path.
    pub fn open_bytes(
        &self,
        file_name: impl Into<String>,
        data: &[u8],
    ) -> Result<DocumentHandle<D>> {
        ensure_document_size(data.len())?;
        let name = file_name.into();
        let file_type_index = file_type_index_for_path(Path::new(&name), D::file_types())
            .or_else(|| default_file_type_index(D::file_types()))
            .ok_or_else(no_file_types::<D>)?;
        let content = Arc::new(D::read(data, &D::file_types()[file_type_index])?);
        Ok(self.make_handle(
            name,
            content.clone(),
            Some(content),
            Some(digest_hex(data)),
            file_type_index,
            false,
            Vec::new(),
        ))
    }

    /// Returns a typed error because browser file access must enter through bytes from a picker.
    pub async fn open(&self, _path: impl AsRef<Path>) -> Result<DocumentHandle<D>> {
        Err(DocumentPlatformError::NativePathUnavailable {
            operation: "DocumentController::open",
        }
        .into())
    }

    /// Opens a document previously saved with [`DocumentHandle::save_stored`].
    pub async fn open_stored(&self, id: &str) -> Result<DocumentHandle<D>> {
        validate_document_id(id)?;
        let store = self.persistent_store()?;
        let bytes = store
            .get(&document_key(id))
            .await
            .context("failed to load the browser document")?
            .ok_or_else(|| DocumentPlatformError::UnknownStoredDocument(id.to_string()))?;
        let (metadata, primary_bytes) = decode_document_envelope(&bytes)?;
        anyhow::ensure!(metadata.id == id, "stored document identifier mismatch");
        let file_type = D::file_types()
            .get(metadata.file_type_index)
            .ok_or_else(|| {
                anyhow!(
                    "invalid stored file type index {}",
                    metadata.file_type_index
                )
            })?;
        let primary_content = Arc::new(D::read(primary_bytes, file_type)?);

        let autosave = store
            .get(&autosave_key(id))
            .await
            .context("failed to load browser document recovery data")?;
        let (content, dirty) = autosave
            .as_deref()
            .and_then(|bytes| decode_autosave_envelope(bytes).ok())
            .filter(|(recovery, _)| recovery.baseline_digest == metadata.digest)
            .and_then(|(_, bytes)| D::read(bytes, file_type).ok())
            .filter(|recovered| recovered != primary_content.as_ref())
            .map_or_else(
                || (primary_content.clone(), false),
                |recovered| (Arc::new(recovered), true),
            );

        Ok(self.make_handle_with_id(
            metadata.name,
            content,
            Some(primary_content),
            Some(metadata.digest),
            Some(id.to_string()),
            metadata.file_type_index,
            dirty,
            metadata.versions,
        ))
    }

    /// Lists origin-scoped documents in most-recently-saved order.
    pub async fn stored_documents(&self) -> Result<Vec<StoredDocument>> {
        let store = self.persistent_store()?;
        let mut documents = Vec::new();
        for key in store
            .keys()
            .await
            .context("failed to list browser documents")?
        {
            let Some(id) = key.strip_prefix(DOCUMENT_KEY_PREFIX) else {
                continue;
            };
            validate_document_id(id)?;
            let Some(bytes) = store
                .get(&key)
                .await
                .context("failed to inspect browser document metadata")?
            else {
                continue;
            };
            let (metadata, _) = decode_document_envelope(&bytes)?;
            documents.push(StoredDocument {
                id: metadata.id,
                name: metadata.name,
                file_type_index: metadata.file_type_index,
                size_bytes: metadata.size_bytes,
                modified_at_millis: metadata.modified_at_millis,
            });
        }
        documents.sort_by(|left, right| {
            right
                .modified_at_millis
                .cmp(&left.modified_at_millis)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(documents)
    }

    /// Deletes a persisted browser document, its recovery snapshot, and its indexed versions.
    pub async fn delete_stored(&self, id: &str) -> Result<bool> {
        validate_document_id(id)?;
        let store = self.persistent_store()?;
        let Some(envelope) = store
            .get(&document_key(id))
            .await
            .context("failed to inspect the browser document before deletion")?
        else {
            return Ok(false);
        };
        let (metadata, _) = decode_document_envelope(&envelope)?;
        let mut keys = metadata
            .versions
            .iter()
            .map(|version| version_key(id, &version.id))
            .collect::<Vec<_>>();
        keys.push(autosave_key(id));
        keys.push(document_key(id));
        let key_refs = keys.iter().map(String::as_str).collect::<Vec<_>>();
        store
            .remove_many(&key_refs)
            .await
            .context("failed to atomically delete the browser document")?;
        Ok(true)
    }

    /// Returns a typed error because browser recents use stable document identifiers, not paths.
    pub fn recent_documents(&self) -> Result<Vec<RecentDocument>> {
        Err(DocumentPlatformError::NativePathUnavailable {
            operation: "DocumentController::recent_documents",
        }
        .into())
    }

    /// Returns a typed error because the browser store has no native-path recent list.
    pub fn clear_recent(&self) -> Result<()> {
        Err(DocumentPlatformError::NativePathUnavailable {
            operation: "DocumentController::clear_recent",
        }
        .into())
    }

    fn persistent_store(&self) -> Result<BlobStore> {
        self.inner
            .store
            .clone()
            .ok_or_else(|| DocumentPlatformError::PersistenceNotConfigured.into())
    }

    fn make_handle(
        &self,
        name: String,
        content: Snapshot<D::Content>,
        saved: Option<Snapshot<D::Content>>,
        digest: Option<String>,
        file_type_index: usize,
        dirty: bool,
        versions: Vec<DocumentVersion>,
    ) -> DocumentHandle<D> {
        self.make_handle_with_id(
            name,
            content,
            saved,
            digest,
            None,
            file_type_index,
            dirty,
            versions,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn make_handle_with_id(
        &self,
        name: String,
        content: Snapshot<D::Content>,
        saved: Option<Snapshot<D::Content>>,
        digest: Option<String>,
        stored_id: Option<String>,
        file_type_index: usize,
        dirty: bool,
        versions: Vec<DocumentVersion>,
    ) -> DocumentHandle<D> {
        DocumentHandle {
            controller: self.inner.clone(),
            state: Arc::new(Mutex::new(DocumentState {
                name,
                content,
                last_saved_snapshot: saved,
                last_saved_digest: digest,
                stored_id,
                file_type_index,
                dirty,
                versions,
                undo_stack: VecDeque::new(),
                redo_stack: VecDeque::new(),
                next_listener_id: 0,
                change_listeners: BTreeMap::new(),
                dirty_listeners: BTreeMap::new(),
                autosave_revision: 0,
                last_autosave_error: None,
            })),
            operation_lock: Arc::new(async_lock::Mutex::new(())),
        }
    }
}

impl<D: Document> DocumentHandle<D> {
    /// Returns a clone of the current document content.
    pub fn content(&self) -> D::Content {
        self.state.lock().content.as_ref().clone()
    }

    /// Returns the user-facing document name.
    pub fn name(&self) -> String {
        self.state.lock().name.clone()
    }

    /// Returns the persistent browser identifier after a successful named save.
    pub fn stored_id(&self) -> Option<String> {
        self.state.lock().stored_id.clone()
    }

    /// Applies an in-memory change and queues an IndexedDB recovery write when possible.
    pub fn modify(&self, f: impl FnOnce(&mut D::Content)) -> Result<()> {
        let (content, change_listeners, dirty_listeners, dirty_changed, dirty) = {
            let mut state = self.state.lock();
            let mut next_content = state.content.as_ref().clone();
            f(&mut next_content);
            if next_content == *state.content {
                return Ok(());
            }

            let next_content = Arc::new(next_content);
            let dirty = state
                .last_saved_snapshot
                .as_ref()
                .is_none_or(|saved| saved.as_ref() != next_content.as_ref());
            let previous = state.content.clone();
            push_history_entry(
                &mut state.undo_stack,
                previous,
                self.controller.max_history_depth,
            );
            state.redo_stack.clear();
            state.content = next_content;
            let dirty_changed = state.dirty != dirty;
            state.dirty = dirty;
            (
                state.content.clone(),
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                dirty_changed,
                dirty,
            )
        };

        notify_change_listeners(&change_listeners, content.as_ref());
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        self.queue_autosave()
    }

    /// Returns whether the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.state.lock().dirty
    }

    /// Always returns `None` because browser documents do not own native paths.
    pub fn file_path(&self) -> Option<PathBuf> {
        None
    }

    /// Serializes the current content using its selected file type.
    pub fn export_bytes(&self) -> Result<DocumentExport> {
        let state = self.state.lock();
        let file_type = selected_file_type::<D>(&state)?;
        let bytes = D::write(state.content.as_ref(), file_type)?;
        ensure_document_size(bytes.len())?;
        Ok(DocumentExport {
            file_name: file_name_with_extension(&state.name, file_type),
            mime_type: file_type.mime,
            bytes,
        })
    }

    /// Serializes the current content using an explicit supported file type.
    pub fn export_as(
        &self,
        file_type_index: usize,
        file_name: impl Into<String>,
    ) -> Result<DocumentExport> {
        let file_type = D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;
        let content = self.state.lock().content.clone();
        let bytes = D::write(content.as_ref(), file_type)?;
        ensure_document_size(bytes.len())?;
        Ok(DocumentExport {
            file_name: file_name_with_extension(&file_name.into(), file_type),
            mime_type: file_type.mime,
            bytes,
        })
    }

    /// Saves to the current persistent browser identifier.
    pub async fn save(&self) -> Result<()> {
        let id = self
            .state
            .lock()
            .stored_id
            .clone()
            .ok_or(DocumentPlatformError::MissingPersistentIdentity)?;
        self.save_stored(&id).await
    }

    /// Returns a typed error because browser downloads are byte-oriented, not native-path writes.
    pub async fn save_as(&self, _path: impl AsRef<Path>) -> Result<()> {
        Err(DocumentPlatformError::NativePathUnavailable {
            operation: "DocumentHandle::save_as",
        }
        .into())
    }

    /// Persists the current document under an application-provided browser identifier.
    pub async fn save_stored(&self, id: &str) -> Result<()> {
        validate_document_id(id)?;
        let operation_lock = self.operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        self.invalidate_queued_autosaves();
        let store = self.persistent_store()?;
        let (content, name, file_type_index, existing_versions) = {
            let state = self.state.lock();
            (
                state.content.clone(),
                state.name.clone(),
                state.file_type_index,
                state.versions.clone(),
            )
        };
        let file_type = D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;
        let bytes = D::write(content.as_ref(), file_type)?;
        ensure_document_size(bytes.len())?;
        let digest = digest_hex(&bytes);
        let modified_at_millis = now_unix_millis();
        let (versions, stale_versions, version) = next_versions(
            existing_versions,
            &digest,
            bytes.len(),
            modified_at_millis,
            self.controller.max_versions,
        );

        let metadata = StoredMetadata {
            id: id.to_string(),
            name,
            file_type_index,
            size_bytes: bytes.len(),
            modified_at_millis,
            digest: digest.clone(),
            versions: versions.clone(),
        };
        let envelope = encode_document_envelope(&metadata, &bytes)?;
        let document_storage_key = document_key(id);
        let version_storage_key = version.as_ref().map(|version| version_key(id, &version.id));
        let mut entries = Vec::with_capacity(2);
        if let Some(version_storage_key) = &version_storage_key {
            entries.push((version_storage_key.as_str(), bytes.as_slice()));
        }
        entries.push((document_storage_key.as_str(), envelope.as_slice()));
        store
            .put_many(&entries)
            .await
            .context("failed to atomically commit the browser document and version")?;
        let _ = store.remove(&autosave_key(id)).await;
        for version in stale_versions {
            let _ = store.remove(&version_key(id, &version.id)).await;
        }

        let (dirty, dirty_changed, dirty_listeners) = {
            let mut state = self.state.lock();
            let dirty_before = state.dirty;
            state.stored_id = Some(id.to_string());
            state.last_saved_snapshot = Some(content);
            state.last_saved_digest = Some(digest);
            state.versions = versions;
            state.dirty = compute_dirty(&state);
            state.last_autosave_error = None;
            (
                state.dirty,
                dirty_before != state.dirty,
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        if dirty {
            self.queue_autosave()?;
        }
        Ok(())
    }

    /// Reverts to the primary bytes from the current persistent browser identifier.
    pub async fn revert(&self) -> Result<()> {
        let operation_lock = self.operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        self.invalidate_queued_autosaves();
        let (id, previous_content, previous_file_type_index) = {
            let state = self.state.lock();
            (
                state
                    .stored_id
                    .clone()
                    .ok_or(DocumentPlatformError::MissingPersistentIdentity)?,
                state.content.clone(),
                state.file_type_index,
            )
        };
        let store = self.persistent_store()?;
        let envelope = store
            .get(&document_key(&id))
            .await
            .context("failed to reload the browser document")?
            .ok_or_else(|| DocumentPlatformError::UnknownStoredDocument(id.clone()))?;
        let (metadata, bytes) = decode_document_envelope(&envelope)?;
        let file_type = D::file_types()
            .get(metadata.file_type_index)
            .ok_or_else(|| {
                anyhow!(
                    "invalid stored file type index {}",
                    metadata.file_type_index
                )
            })?;
        let content = Arc::new(D::read(bytes, file_type)?);
        let (was_dirty, change_listeners, dirty_listeners) = {
            let mut state = self.state.lock();
            anyhow::ensure!(
                Arc::ptr_eq(&state.content, &previous_content)
                    && state.stored_id.as_deref() == Some(id.as_str())
                    && state.file_type_index == previous_file_type_index,
                "document changed while browser revert was in progress"
            );
            let was_dirty = state.dirty;
            state.content = content.clone();
            state.last_saved_snapshot = Some(content.clone());
            state.last_saved_digest = Some(metadata.digest);
            state.file_type_index = metadata.file_type_index;
            state.versions = metadata.versions;
            state.dirty = false;
            state.undo_stack.clear();
            state.redo_stack.clear();
            state.last_autosave_error = None;
            (
                was_dirty,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };
        let _ = store.remove(&autosave_key(&id)).await;
        notify_change_listeners(&change_listeners, content.as_ref());
        if was_dirty {
            notify_dirty_listeners(&dirty_listeners, false);
        }
        Ok(())
    }

    /// Restores the previous in-memory snapshot if one exists.
    pub fn undo(&self) -> Result<()> {
        self.restore_history_entry(true)
    }

    /// Restores the next redo snapshot if one exists.
    pub fn redo(&self) -> Result<()> {
        self.restore_history_entry(false)
    }

    /// Returns whether an undo operation is currently available.
    pub fn can_undo(&self) -> bool {
        !self.state.lock().undo_stack.is_empty()
    }

    /// Returns whether a redo operation is currently available.
    pub fn can_redo(&self) -> bool {
        !self.state.lock().redo_stack.is_empty()
    }

    /// Returns the version metadata loaded with the current persistent document.
    pub fn versions(&self) -> Result<Vec<DocumentVersion>> {
        if self.state.lock().stored_id.is_none() {
            return Err(DocumentPlatformError::MissingPersistentIdentity.into());
        }
        Ok(self.state.lock().versions.clone())
    }

    /// Restores a persisted IndexedDB version without saving it as the primary revision.
    pub async fn restore_version(&self, version: &DocumentVersion) -> Result<()> {
        let operation_lock = self.operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        let (id, file_type_index, previous_content, known) = {
            let state = self.state.lock();
            (
                state
                    .stored_id
                    .clone()
                    .ok_or(DocumentPlatformError::MissingPersistentIdentity)?,
                state.file_type_index,
                state.content.clone(),
                state.versions.iter().any(|known| known == version),
            )
        };
        anyhow::ensure!(known, "unknown browser document version {}", version.id);
        let bytes = self
            .persistent_store()?
            .get(&version_key(&id, &version.id))
            .await
            .context("failed to read the browser document version")?
            .ok_or_else(|| anyhow!("missing browser document version {}", version.id))?;
        anyhow::ensure!(
            bytes.len() == version.size_bytes && digest_hex(&bytes) == version.digest,
            "browser document version integrity check failed"
        );
        let file_type = D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;
        let content = Arc::new(D::read(&bytes, file_type)?);
        self.replace_content_from_history(content, Some(&previous_content))?;
        self.queue_autosave()
    }

    /// Waits for the latest recovery snapshot to commit to IndexedDB.
    pub async fn flush_autosave(&self) -> Result<()> {
        let operation_lock = self.operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        let store = self.persistent_store()?;
        let (id, dirty, payload) = {
            let state = self.state.lock();
            let id = state
                .stored_id
                .clone()
                .ok_or(DocumentPlatformError::MissingPersistentIdentity)?;
            let payload = if state.dirty {
                Some(encode_current_autosave::<D>(&state)?)
            } else {
                None
            };
            (id, state.dirty, payload)
        };
        let result = if dirty {
            store.put(&autosave_key(&id), &payload.unwrap()).await
        } else {
            store.remove(&autosave_key(&id)).await.map(|_| ())
        };
        match result {
            Ok(()) => {
                self.state.lock().last_autosave_error = None;
                Ok(())
            }
            Err(error) => {
                self.state.lock().last_autosave_error = Some(error.to_string());
                Err(error).context("failed to flush browser document recovery data")
            }
        }
    }

    /// Returns the most recent asynchronous recovery-write error, if any.
    pub fn last_autosave_error(&self) -> Option<String> {
        self.state.lock().last_autosave_error.clone()
    }

    /// Registers a listener that fires when the document content changes.
    pub fn on_change(
        &self,
        callback: impl Fn(&D::Content) + Send + Sync + 'static,
    ) -> Subscription {
        let state = self.state.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.allocate_listener_id();
            state
                .change_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };
        Subscription::new(move || {
            state.lock().change_listeners.remove(&listener_id);
        })
    }

    /// Registers a listener that fires when the dirty state changes.
    pub fn on_dirty_change(&self, callback: impl Fn(bool) + Send + Sync + 'static) -> Subscription {
        let state = self.state.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.allocate_listener_id();
            state
                .dirty_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };
        Subscription::new(move || {
            state.lock().dirty_listeners.remove(&listener_id);
        })
    }

    fn persistent_store(&self) -> Result<BlobStore> {
        self.controller
            .store
            .clone()
            .ok_or_else(|| DocumentPlatformError::PersistenceNotConfigured.into())
    }

    fn restore_history_entry(&self, undo: bool) -> Result<()> {
        let next = {
            let state = self.state.lock();
            if undo {
                state.undo_stack.back().cloned()
            } else {
                state.redo_stack.back().cloned()
            }
        };
        let Some(next) = next else {
            return Ok(());
        };

        let (content, dirty, dirty_changed, change_listeners, dirty_listeners) = {
            let mut state = self.state.lock();
            let current = state.content.clone();
            if undo {
                state.undo_stack.pop_back();
                state.redo_stack.push_back(current);
            } else {
                state.redo_stack.pop_back();
                state.undo_stack.push_back(current);
            }
            state.content = next;
            let dirty = compute_dirty(&state);
            let dirty_changed = state.dirty != dirty;
            state.dirty = dirty;
            (
                state.content.clone(),
                dirty,
                dirty_changed,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };
        notify_change_listeners(&change_listeners, content.as_ref());
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        self.queue_autosave()
    }

    fn replace_content_from_history(
        &self,
        content: Snapshot<D::Content>,
        expected_content: Option<&Snapshot<D::Content>>,
    ) -> Result<()> {
        let (content, dirty, dirty_changed, change_listeners, dirty_listeners) = {
            let mut state = self.state.lock();
            if let Some(expected_content) = expected_content {
                anyhow::ensure!(
                    Arc::ptr_eq(&state.content, expected_content),
                    "document changed while browser version restore was in progress"
                );
            }
            if state.content.as_ref() == content.as_ref() {
                return Ok(());
            }
            let previous = state.content.clone();
            push_history_entry(
                &mut state.undo_stack,
                previous,
                self.controller.max_history_depth,
            );
            state.redo_stack.clear();
            state.content = content.clone();
            let dirty = compute_dirty(&state);
            let dirty_changed = state.dirty != dirty;
            state.dirty = dirty;
            (
                content,
                dirty,
                dirty_changed,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };
        notify_change_listeners(&change_listeners, content.as_ref());
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        Ok(())
    }

    fn invalidate_queued_autosaves(&self) {
        let mut state = self.state.lock();
        state.autosave_revision = state.autosave_revision.wrapping_add(1);
    }

    fn queue_autosave(&self) -> Result<()> {
        let Some(store) = self.controller.store.clone() else {
            return Ok(());
        };
        let (id, dirty, payload, revision) = {
            let mut state = self.state.lock();
            let Some(id) = state.stored_id.clone() else {
                return Ok(());
            };
            let payload = if state.dirty {
                Some(encode_current_autosave::<D>(&state)?)
            } else {
                None
            };
            state.autosave_revision = state.autosave_revision.wrapping_add(1);
            (id, state.dirty, payload, state.autosave_revision)
        };
        let state = self.state.clone();
        let operation_lock = self.operation_lock.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _guard = operation_lock.lock().await;
            if state.lock().autosave_revision != revision {
                return;
            }
            let result = if dirty {
                store.put(&autosave_key(&id), &payload.unwrap()).await
            } else {
                store.remove(&autosave_key(&id)).await.map(|_| ())
            };
            let mut state = state.lock();
            if state.autosave_revision == revision {
                state.last_autosave_error = result.err().map(|error| error.to_string());
            }
        });
        Ok(())
    }
}

fn selected_file_type<D: Document>(state: &DocumentState<D>) -> Result<&'static FileType> {
    D::file_types()
        .get(state.file_type_index)
        .or_else(|| D::file_types().first())
        .ok_or_else(no_file_types::<D>)
}

fn no_file_types<D: Document>() -> anyhow::Error {
    anyhow!(
        "document type {} does not define any file types",
        std::any::type_name::<D>()
    )
}

fn compute_dirty<D: Document>(state: &DocumentState<D>) -> bool {
    state
        .last_saved_snapshot
        .as_ref()
        .is_none_or(|saved| saved.as_ref() != state.content.as_ref())
}

fn push_history_entry<T>(history: &mut VecDeque<T>, entry: T, max_depth: usize) {
    history.push_back(entry);
    while history.len() > max_depth {
        history.pop_front();
    }
}

fn notify_change_listeners<T>(listeners: &[ChangeListener<T>], content: &T) {
    for listener in listeners {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(content)));
    }
}

fn notify_dirty_listeners(listeners: &[DirtyListener], dirty: bool) {
    for listener in listeners {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(dirty)));
    }
}

fn ensure_document_size(size: usize) -> Result<()> {
    anyhow::ensure!(
        u64::try_from(size).unwrap_or(u64::MAX) <= MAX_DOCUMENT_BYTES,
        "document exceeds the {MAX_DOCUMENT_BYTES} byte limit"
    );
    Ok(())
}

fn validate_document_id(id: &str) -> Result<()> {
    let valid =
        !id.is_empty() && id.len() <= MAX_DOCUMENT_ID_BYTES && !id.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err(DocumentPlatformError::InvalidDocumentIdentifier(id.to_string()).into())
    }
}

fn file_name_with_extension(name: &str, file_type: &FileType) -> String {
    let extension = file_type
        .extensions
        .iter()
        .map(|extension| extension.trim_start_matches('.'))
        .find(|extension| !extension.is_empty());
    let Some(extension) = extension else {
        return name.to_string();
    };
    if Path::new(name)
        .extension()
        .and_then(|candidate| candidate.to_str())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
    {
        name.to_string()
    } else {
        format!("{name}.{extension}")
    }
}

fn encode_document_envelope(metadata: &StoredMetadata, bytes: &[u8]) -> Result<Vec<u8>> {
    ensure_document_size(bytes.len())?;
    anyhow::ensure!(metadata.size_bytes == bytes.len(), "document size mismatch");
    anyhow::ensure!(
        metadata.digest == digest_hex(bytes),
        "document digest mismatch"
    );
    let metadata =
        serde_json::to_vec(metadata).context("failed to encode browser document metadata")?;
    anyhow::ensure!(
        metadata.len() <= MAX_ENVELOPE_METADATA_BYTES,
        "browser document metadata exceeds its size limit"
    );
    let metadata_len =
        u32::try_from(metadata.len()).context("browser document metadata is too large")?;
    let mut envelope = Vec::with_capacity(DOCUMENT_MAGIC.len() + 5 + metadata.len() + bytes.len());
    envelope.extend_from_slice(DOCUMENT_MAGIC);
    envelope.push(DOCUMENT_VERSION);
    envelope.extend_from_slice(&metadata_len.to_le_bytes());
    envelope.extend_from_slice(&metadata);
    envelope.extend_from_slice(bytes);
    ensure_document_size(envelope.len())?;
    Ok(envelope)
}

fn decode_document_envelope(envelope: &[u8]) -> Result<(StoredMetadata, &[u8])> {
    ensure_document_size(envelope.len())?;
    let remainder = envelope
        .strip_prefix(DOCUMENT_MAGIC)
        .ok_or(DocumentPlatformError::InvalidStoredDocument)?;
    let (&version, remainder) = remainder
        .split_first()
        .ok_or(DocumentPlatformError::InvalidStoredDocument)?;
    if version != DOCUMENT_VERSION || remainder.len() < 4 {
        return Err(DocumentPlatformError::InvalidStoredDocument.into());
    }
    let metadata_len = usize::try_from(u32::from_le_bytes(
        remainder[..4]
            .try_into()
            .map_err(|_| DocumentPlatformError::InvalidStoredDocument)?,
    ))
    .map_err(|_| DocumentPlatformError::InvalidStoredDocument)?;
    if metadata_len > MAX_ENVELOPE_METADATA_BYTES || remainder.len() < 4 + metadata_len {
        return Err(DocumentPlatformError::InvalidStoredDocument.into());
    }
    let metadata: StoredMetadata = serde_json::from_slice(&remainder[4..4 + metadata_len])
        .map_err(|_| DocumentPlatformError::InvalidStoredDocument)?;
    let bytes = &remainder[4 + metadata_len..];
    validate_document_id(&metadata.id)?;
    validate_versions(&metadata.versions)?;
    anyhow::ensure!(
        metadata.size_bytes == bytes.len(),
        "stored document size mismatch"
    );
    anyhow::ensure!(
        metadata.digest == digest_hex(bytes),
        "stored document digest mismatch"
    );
    Ok((metadata, bytes))
}

fn encode_current_autosave<D: Document>(state: &DocumentState<D>) -> Result<Vec<u8>> {
    let file_type = selected_file_type::<D>(state)?;
    let bytes = D::write(state.content.as_ref(), file_type)?;
    ensure_document_size(bytes.len())?;
    let metadata = AutosaveMetadata {
        baseline_digest: state.last_saved_digest.clone().unwrap_or_default(),
        size_bytes: bytes.len(),
    };
    let metadata =
        serde_json::to_vec(&metadata).context("failed to encode browser autosave metadata")?;
    let metadata_len =
        u32::try_from(metadata.len()).context("browser autosave metadata is too large")?;
    let mut envelope = Vec::with_capacity(AUTOSAVE_MAGIC.len() + 5 + metadata.len() + bytes.len());
    envelope.extend_from_slice(AUTOSAVE_MAGIC);
    envelope.push(AUTOSAVE_VERSION);
    envelope.extend_from_slice(&metadata_len.to_le_bytes());
    envelope.extend_from_slice(&metadata);
    envelope.extend_from_slice(&bytes);
    ensure_document_size(envelope.len())?;
    Ok(envelope)
}

fn decode_autosave_envelope(envelope: &[u8]) -> Result<(AutosaveMetadata, &[u8])> {
    ensure_document_size(envelope.len())?;
    let remainder = envelope
        .strip_prefix(AUTOSAVE_MAGIC)
        .ok_or(DocumentPlatformError::InvalidStoredDocument)?;
    let (&version, remainder) = remainder
        .split_first()
        .ok_or(DocumentPlatformError::InvalidStoredDocument)?;
    if version != AUTOSAVE_VERSION || remainder.len() < 4 {
        return Err(DocumentPlatformError::InvalidStoredDocument.into());
    }
    let metadata_len = usize::try_from(u32::from_le_bytes(
        remainder[..4]
            .try_into()
            .map_err(|_| DocumentPlatformError::InvalidStoredDocument)?,
    ))
    .map_err(|_| DocumentPlatformError::InvalidStoredDocument)?;
    if metadata_len > MAX_ENVELOPE_METADATA_BYTES || remainder.len() < 4 + metadata_len {
        return Err(DocumentPlatformError::InvalidStoredDocument.into());
    }
    let metadata: AutosaveMetadata = serde_json::from_slice(&remainder[4..4 + metadata_len])
        .map_err(|_| DocumentPlatformError::InvalidStoredDocument)?;
    let bytes = &remainder[4 + metadata_len..];
    anyhow::ensure!(metadata.size_bytes == bytes.len(), "autosave size mismatch");
    Ok((metadata, bytes))
}

fn next_versions(
    mut versions: Vec<DocumentVersion>,
    digest: &str,
    size_bytes: usize,
    timestamp: u64,
    max_versions: usize,
) -> (
    Vec<DocumentVersion>,
    Vec<DocumentVersion>,
    Option<DocumentVersion>,
) {
    if versions
        .last()
        .is_some_and(|version| version.digest == digest)
    {
        return (versions, Vec::new(), None);
    }
    let timestamp = versions
        .last()
        .map_or(timestamp, |last| timestamp.max(last.created_at_millis));
    static NEXT_VERSION_ID: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT_VERSION_ID.fetch_add(1, Ordering::Relaxed);
    let version = DocumentVersion {
        id: format!("{timestamp}-{}-{sequence}", &digest[..digest.len().min(16)]),
        created_at_millis: timestamp,
        digest: digest.to_string(),
        size_bytes,
    };
    versions.push(version.clone());
    let stale_count = versions.len().saturating_sub(max_versions.max(1));
    let stale = versions.drain(..stale_count).collect();
    (versions, stale, Some(version))
}

fn validate_versions(versions: &[DocumentVersion]) -> Result<()> {
    anyhow::ensure!(versions.len() <= 20, "too many stored document versions");
    let mut previous_timestamp = None;
    for version in versions {
        anyhow::ensure!(
            !version.id.is_empty()
                && version.id.len() <= 256
                && !version.id.chars().any(char::is_control),
            "invalid browser document version identifier"
        );
        anyhow::ensure!(
            version.digest.len() == 64
                && version
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "invalid browser document version digest"
        );
        ensure_document_size(version.size_bytes)?;
        if let Some(previous_timestamp) = previous_timestamp {
            anyhow::ensure!(
                version.created_at_millis >= previous_timestamp,
                "browser document versions are not ordered"
            );
        }
        previous_timestamp = Some(version.created_at_millis);
    }
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_unix_millis() -> u64 {
    let millis = js_sys::Date::now();
    if millis.is_finite() && millis >= 0.0 {
        millis as u64
    } else {
        0
    }
}

fn document_key(id: &str) -> String {
    format!("{DOCUMENT_KEY_PREFIX}{id}")
}

fn autosave_key(id: &str) -> String {
    format!("{AUTOSAVE_KEY_PREFIX}{id}")
}

fn version_key(id: &str, version_id: &str) -> String {
    format!("{VERSION_KEY_PREFIX}{id}:{version_id}")
}

#[cfg(test)]
mod tests {
    use super::{
        Document, DocumentController, DocumentPlatformError, FileType, StoredMetadata,
        decode_document_envelope, encode_document_envelope,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    struct TextDocument;

    const TYPES: &[FileType] = &[FileType {
        name: "Text",
        extensions: &["txt"],
        uti: None,
        mime: Some("text/plain"),
    }];

    impl Document for TextDocument {
        type Content = String;

        fn file_types() -> &'static [FileType] {
            TYPES
        }

        fn new_untitled() -> Self::Content {
            String::new()
        }

        fn read(data: &[u8], _file_type: &FileType) -> crate::Result<Self::Content> {
            Ok(String::from_utf8(data.to_vec())?)
        }

        fn write(content: &Self::Content, _file_type: &FileType) -> crate::Result<Vec<u8>> {
            Ok(content.as_bytes().to_vec())
        }
    }

    #[test]
    fn byte_import_edit_undo_and_export_do_not_need_dom_storage() {
        let controller = DocumentController::<TextDocument>::new("com.example.docs").unwrap();
        let document = controller.open_bytes("notes.txt", b"hello").unwrap();
        document.modify(|text| text.push_str(" web")).unwrap();
        assert_eq!(document.export_bytes().unwrap().bytes, b"hello web");
        document.undo().unwrap();
        assert_eq!(document.content(), "hello");
    }

    #[test]
    fn envelope_round_trip_checks_content_integrity() {
        let bytes = b"document";
        let metadata = StoredMetadata {
            id: "doc-1".into(),
            name: "Document".into(),
            file_type_index: 0,
            size_bytes: bytes.len(),
            modified_at_millis: 1,
            digest: super::digest_hex(bytes),
            versions: Vec::new(),
        };
        let envelope = encode_document_envelope(&metadata, bytes).unwrap();
        let (decoded, decoded_bytes) = decode_document_envelope(&envelope).unwrap();
        assert_eq!(decoded.id, "doc-1");
        assert_eq!(decoded_bytes, bytes);

        let mut corrupt = envelope;
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(decode_document_envelope(&corrupt).is_err());
    }

    #[test]
    fn native_paths_fail_with_a_typed_boundary_error() {
        let error = DocumentController::<TextDocument>::new_in("app", "/tmp")
            .err()
            .unwrap();
        assert!(matches!(
            error.downcast_ref::<DocumentPlatformError>(),
            Some(DocumentPlatformError::NativePathUnavailable { .. })
        ));
    }

    #[wasm_bindgen_test]
    async fn indexed_db_documents_survive_reopen_with_versions_and_recovery() {
        let app_id = format!("kael.document.test.{}", js_sys::Date::now());
        let controller = DocumentController::<TextDocument>::new_persistent(&app_id)
            .await
            .unwrap();
        let document = controller.open_bytes("notes.txt", b"first").unwrap();
        document.save_stored("notes").await.unwrap();
        document.modify(|text| text.push_str(" draft")).unwrap();
        document.flush_autosave().await.unwrap();

        let reopened = DocumentController::<TextDocument>::new_persistent(&app_id)
            .await
            .unwrap()
            .open_stored("notes")
            .await
            .unwrap();
        assert_eq!(reopened.content(), "first draft");
        assert!(reopened.is_dirty());
        reopened.save().await.unwrap();
        assert!(!reopened.versions().unwrap().is_empty());

        let listed = controller.stored_documents().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "notes");
        assert!(controller.delete_stored("notes").await.unwrap());
    }
}
