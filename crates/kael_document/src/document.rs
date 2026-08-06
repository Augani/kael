//! Document controller and handle types.

use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use parking_lot::Mutex;

use crate::{
    AutosaveConfig, FileType, RecentDocument, Subscription, autosave,
    file_type::{default_file_type_index, file_type_index_for_path},
    recent::RecentDocumentStore,
    versions::{DocumentVersion, VersionStore},
};

type ChangeListener<T> = Arc<dyn Fn(&T) + Send + Sync + 'static>;
type DirtyListener = Arc<dyn Fn(bool) + Send + Sync + 'static>;
type Snapshot<T> = Arc<T>;

/// A document format supported by the controller.
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

/// A controller that manages documents of a specific type.
pub struct DocumentController<D: Document> {
    inner: Arc<ControllerState<D>>,
}

/// A mutable document handle managed by a controller.
pub struct DocumentHandle<D: Document> {
    controller: Arc<ControllerState<D>>,
    state: Arc<Mutex<DocumentState<D>>>,
    file_operation_lock: Arc<smol::lock::Mutex<()>>,
    recovery_lock: Arc<Mutex<()>>,
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
            file_operation_lock: self.file_operation_lock.clone(),
            recovery_lock: self.recovery_lock.clone(),
        }
    }
}

struct ControllerState<D: Document> {
    app_id: String,
    autosave_config: AutosaveConfig,
    recent_store: RecentDocumentStore,
    version_store: VersionStore,
    max_history_depth: usize,
    metadata_lock: Arc<Mutex<()>>,
    _marker: PhantomData<fn() -> D>,
}

impl<D: Document> Clone for ControllerState<D> {
    fn clone(&self) -> Self {
        Self {
            app_id: self.app_id.clone(),
            autosave_config: self.autosave_config.clone(),
            recent_store: self.recent_store.clone(),
            version_store: self.version_store.clone(),
            max_history_depth: self.max_history_depth,
            metadata_lock: self.metadata_lock.clone(),
            _marker: PhantomData,
        }
    }
}

struct DocumentState<D: Document> {
    name: String,
    content: Snapshot<D::Content>,
    last_saved_snapshot: Option<Snapshot<D::Content>>,
    last_saved_digest: Option<autosave::ContentDigest>,
    file_path: Option<PathBuf>,
    file_type_index: usize,
    dirty: bool,
    autosave_path: PathBuf,
    undo_stack: VecDeque<Snapshot<D::Content>>,
    redo_stack: VecDeque<Snapshot<D::Content>>,
    next_listener_id: usize,
    change_listeners: BTreeMap<usize, ChangeListener<D::Content>>,
    dirty_listeners: BTreeMap<usize, DirtyListener>,
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

impl<D: Document> DocumentController<D> {
    /// Creates a controller rooted in the platform-standard data directory.
    pub fn new(app_id: impl Into<String>) -> Result<Self> {
        let app_id = app_id.into();
        let root = crate::platform::document_storage_root(&app_id)?;
        Self::new_in(app_id, root)
    }

    /// Creates a controller rooted at an explicit storage directory.
    pub fn new_in(app_id: impl Into<String>, storage_root: impl AsRef<Path>) -> Result<Self> {
        let app_id = app_id.into();
        let storage_root = storage_root.as_ref();
        std::fs::create_dir_all(storage_root).with_context(|| {
            format!(
                "failed to create document storage root {}",
                storage_root.display()
            )
        })?;
        let storage_metadata = std::fs::symlink_metadata(storage_root).with_context(|| {
            format!(
                "failed to inspect document storage root {}",
                storage_root.display()
            )
        })?;
        anyhow::ensure!(
            storage_metadata.file_type().is_dir(),
            "document storage root {} is not a real directory",
            storage_root.display()
        );

        Ok(Self {
            inner: Arc::new(ControllerState {
                app_id,
                autosave_config: AutosaveConfig::default(),
                recent_store: RecentDocumentStore::new_in(storage_root, 50)?,
                version_store: VersionStore::new_in(storage_root, 20)?,
                max_history_depth: 100,
                metadata_lock: Arc::new(Mutex::new(())),
                _marker: PhantomData,
            }),
        })
    }

    /// Returns a controller configured with a custom autosave strategy.
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
        let autosave_path = autosave::autosave_path(
            &self.inner.app_id,
            &self.inner.autosave_config.location,
            None,
            &name,
        );
        let content = Arc::new(D::new_untitled());

        DocumentHandle {
            controller: self.inner.clone(),
            state: Arc::new(Mutex::new(DocumentState {
                name,
                content: content.clone(),
                last_saved_snapshot: Some(content),
                last_saved_digest: None,
                file_path: None,
                file_type_index,
                dirty: false,
                autosave_path,
                undo_stack: VecDeque::new(),
                redo_stack: VecDeque::new(),
                next_listener_id: 0,
                change_listeners: BTreeMap::new(),
                dirty_listeners: BTreeMap::new(),
            })),
            file_operation_lock: Arc::new(smol::lock::Mutex::new(())),
            recovery_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Opens an existing document.
    pub async fn open(&self, path: impl AsRef<Path>) -> Result<DocumentHandle<D>> {
        let path = normalize_path(path.as_ref())?;
        let controller = self.inner.clone();
        let (
            path,
            name,
            content,
            saved_content,
            saved_digest,
            file_type_index,
            dirty,
            autosave_path,
        ) = smol::unblock(move || -> Result<_> {
            let file_types = D::file_types();
            let file_type_index = file_type_index_for_path(&path, file_types)
                .or_else(|| default_file_type_index(file_types))
                .ok_or_else(|| {
                    anyhow!(
                        "document type {} does not define any file types",
                        std::any::type_name::<D>()
                    )
                })?;
            let file_type = &file_types[file_type_index];
            let saved_bytes = read_document_bytes(&path)?;
            let saved_digest = autosave::content_digest(&saved_bytes);
            let saved_content = Arc::new(D::read(&saved_bytes, file_type)?);
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("document")
                .to_string();
            let autosave_path = autosave::autosave_path(
                &controller.app_id,
                &controller.autosave_config.location,
                Some(&path),
                &name,
            );

            let (content, dirty) = match autosave::load_autosave(&autosave_path) {
                Ok(Some(snapshot)) => {
                    let compatible = snapshot.baseline_digest.as_ref() == Some(&saved_digest)
                        || (snapshot.legacy
                            && autosave::legacy_snapshot_is_newer(&autosave_path, &path)
                                .unwrap_or(false));
                    if compatible
                        && let Ok(autosave_content) = D::read(&snapshot.bytes, file_type)
                        && autosave_content != *saved_content
                    {
                        (Arc::new(autosave_content), true)
                    } else {
                        let _ = autosave::clear_autosave(&autosave_path);
                        (saved_content.clone(), false)
                    }
                }
                Ok(None) => (saved_content.clone(), false),
                Err(_) => {
                    let _ = autosave::clear_autosave(&autosave_path);
                    (saved_content.clone(), false)
                }
            };

            // Recent-document tracking is ancillary and must not make an otherwise valid
            // document impossible to open. Callers can still inspect or clear the store.
            let _metadata_guard = controller.metadata_lock.lock();
            let _ = controller.recent_store.record(&path);

            Ok((
                path,
                name,
                content,
                saved_content,
                saved_digest,
                file_type_index,
                dirty,
                autosave_path,
            ))
        })
        .await?;

        Ok(DocumentHandle {
            controller: self.inner.clone(),
            state: Arc::new(Mutex::new(DocumentState {
                name,
                content,
                last_saved_snapshot: Some(saved_content),
                last_saved_digest: Some(saved_digest),
                file_path: Some(path),
                file_type_index,
                dirty,
                autosave_path,
                undo_stack: VecDeque::new(),
                redo_stack: VecDeque::new(),
                next_listener_id: 0,
                change_listeners: BTreeMap::new(),
                dirty_listeners: BTreeMap::new(),
            })),
            file_operation_lock: Arc::new(smol::lock::Mutex::new(())),
            recovery_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Returns the recent documents tracked by this controller.
    pub fn recent_documents(&self) -> Result<Vec<RecentDocument>> {
        let _metadata_guard = self.inner.metadata_lock.lock();
        self.inner.recent_store.load()
    }

    /// Clears the stored recent-document list.
    pub fn clear_recent(&self) -> Result<()> {
        let _metadata_guard = self.inner.metadata_lock.lock();
        self.inner.recent_store.clear()
    }
}

impl<D: Document> DocumentHandle<D> {
    /// Returns a clone of the current document content.
    pub fn content(&self) -> D::Content {
        self.state.lock().content.as_ref().clone()
    }

    /// Applies an in-memory change to the document content.
    pub fn modify(&self, f: impl FnOnce(&mut D::Content)) -> Result<()> {
        let (content, change_listeners, dirty_listeners, dirty_changed, dirty, autosave_result) = {
            let _recovery_guard = self.recovery_lock.lock();
            let mut state = self.state.lock();
            let mut next_content = state.content.as_ref().clone();
            f(&mut next_content);

            if next_content == *state.content {
                return Ok(());
            }

            let file_type = *selected_file_type::<D>(&state)?;
            let next_content = Arc::new(next_content);
            let dirty = state
                .last_saved_snapshot
                .as_ref()
                .is_none_or(|saved| saved.as_ref() != next_content.as_ref());

            let previous_content = state.content.clone();
            push_history_entry(
                &mut state.undo_stack,
                previous_content,
                self.controller.max_history_depth,
            );
            state.redo_stack.clear();
            state.content = next_content;

            let content = state.content.clone();
            let previous_dirty = state.dirty;
            state.dirty = dirty;
            let autosave_result: Result<()> = (|| {
                if dirty {
                    let autosave_bytes = D::write(state.content.as_ref(), &file_type)?;
                    autosave::write_autosave(
                        &state.autosave_path,
                        &autosave_bytes,
                        state.last_saved_digest.as_ref(),
                    )
                } else {
                    autosave::clear_autosave(&state.autosave_path)
                }
            })();

            (
                content,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                previous_dirty != state.dirty,
                state.dirty,
                autosave_result,
            )
        };

        notify_change_listeners(&change_listeners, content.as_ref());
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        autosave_result.context(
            "document was modified, but its autosave recovery snapshot could not be updated",
        )
    }

    /// Returns whether the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.state.lock().dirty
    }

    /// Returns the current on-disk path for the document if one exists.
    pub fn file_path(&self) -> Option<PathBuf> {
        self.state.lock().file_path.clone()
    }

    /// Saves the document back to its current path.
    pub async fn save(&self) -> Result<()> {
        let operation_lock = self.file_operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        let path = self
            .state
            .lock()
            .file_path
            .clone()
            .ok_or_else(|| anyhow!("document has not been saved to a path yet"))?;
        self.save_to_path(path, false).await
    }

    /// Saves the document to a new path.
    pub async fn save_as(&self, path: impl AsRef<Path>) -> Result<()> {
        let operation_lock = self.file_operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        self.save_to_path(normalize_path(path.as_ref())?, true)
            .await
    }

    /// Reverts the document to the most recently saved on-disk version.
    pub async fn revert(&self) -> Result<()> {
        let operation_lock = self.file_operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        let (path, file_type_index, previous_content, autosave_path) = {
            let state = self.state.lock();
            (
                state
                    .file_path
                    .clone()
                    .ok_or_else(|| anyhow!("document has not been saved to a path yet"))?,
                state.file_type_index,
                state.content.clone(),
                state.autosave_path.clone(),
            )
        };

        let file_type = D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;
        let path_for_read = path.clone();
        let (content, saved_digest) = smol::unblock(move || {
            let bytes = read_document_bytes(&path_for_read)?;
            let digest = autosave::content_digest(&bytes);
            Ok::<_, anyhow::Error>((Arc::new(D::read(&bytes, file_type)?), digest))
        })
        .await?;

        let (previous_dirty, change_listeners, dirty_listeners) = {
            let _recovery_guard = self.recovery_lock.lock();
            let mut state = self.state.lock();
            anyhow::ensure!(
                Arc::ptr_eq(&state.content, &previous_content)
                    && state.file_path.as_ref() == Some(&path)
                    && state.file_type_index == file_type_index,
                "document changed while revert was in progress"
            );
            autosave::clear_autosave(&autosave_path)?;
            let previous_dirty = state.dirty;
            state.content = content.clone();
            state.last_saved_snapshot = Some(content.clone());
            state.last_saved_digest = Some(saved_digest);
            state.dirty = false;
            state.undo_stack.clear();
            state.redo_stack.clear();
            (
                previous_dirty,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };

        notify_change_listeners(&change_listeners, content.as_ref());
        if previous_dirty {
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

    /// Returns the persisted versions for the current file-backed document.
    pub fn versions(&self) -> Result<Vec<DocumentVersion>> {
        let document_key = self.document_key()?;
        let _metadata_guard = self.controller.metadata_lock.lock();
        self.controller.version_store.load(&document_key)
    }

    /// Restores a persisted version into the current document without saving it.
    pub async fn restore_version(&self, version: &DocumentVersion) -> Result<()> {
        let operation_lock = self.file_operation_lock.clone();
        let _operation_guard = operation_lock.lock().await;
        let (document_key, path, file_type_index, previous_content) = {
            let state = self.state.lock();
            let path = state
                .file_path
                .clone()
                .ok_or_else(|| anyhow!("document does not have a file path yet"))?;
            (
                document_key(&path),
                path,
                state.file_type_index,
                state.content.clone(),
            )
        };
        let file_type = *D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;
        let version_store = self.controller.version_store.clone();
        let metadata_lock = self.controller.metadata_lock.clone();
        let version = version.clone();
        let content = smol::unblock(move || {
            let _metadata_guard = metadata_lock.lock();
            let bytes = version_store.read(&document_key, &version)?;
            Ok::<_, anyhow::Error>(Arc::new(D::read(&bytes, &file_type)?))
        })
        .await?;
        let (content, change_listeners, dirty_listeners, dirty_changed, dirty, autosave_result) = {
            let _recovery_guard = self.recovery_lock.lock();
            let mut state = self.state.lock();
            anyhow::ensure!(
                Arc::ptr_eq(&state.content, &previous_content)
                    && state.file_path.as_ref() == Some(&path)
                    && state.file_type_index == file_type_index,
                "document changed while version restore was in progress"
            );

            if content.as_ref() == state.content.as_ref() {
                return Ok(());
            }

            let dirty = state
                .last_saved_snapshot
                .as_ref()
                .is_none_or(|saved| saved.as_ref() != content.as_ref());
            let previous_content = state.content.clone();
            push_history_entry(
                &mut state.undo_stack,
                previous_content,
                self.controller.max_history_depth,
            );
            state.redo_stack.clear();
            state.content = content.clone();
            let previous_dirty = state.dirty;
            state.dirty = dirty;
            let autosave_result = (|| {
                if dirty {
                    let autosave_bytes = D::write(state.content.as_ref(), &file_type)?;
                    autosave::write_autosave(
                        &state.autosave_path,
                        &autosave_bytes,
                        state.last_saved_digest.as_ref(),
                    )
                } else {
                    autosave::clear_autosave(&state.autosave_path)
                }
            })();

            (
                content,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                previous_dirty != state.dirty,
                state.dirty,
                autosave_result,
            )
        };

        notify_change_listeners(&change_listeners, content.as_ref());
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        autosave_result.context(
            "document version was restored, but its autosave recovery snapshot could not be updated",
        )
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

    async fn save_to_path(&self, path: PathBuf, update_file_type: bool) -> Result<()> {
        let normalized_path = normalize_path(&path)?;
        let file_type_index = if update_file_type {
            file_type_index_for_path(&normalized_path, D::file_types())
                .or_else(|| default_file_type_index(D::file_types()))
                .ok_or_else(|| {
                    anyhow!(
                        "document type {} does not define any file types",
                        std::any::type_name::<D>()
                    )
                })?
        } else {
            self.state.lock().file_type_index
        };
        let file_type = D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;

        let (content, current_name, old_autosave_path) = {
            let state = self.state.lock();
            (
                state.content.clone(),
                state.name.clone(),
                state.autosave_path.clone(),
            )
        };

        let new_autosave_path = autosave::autosave_path(
            &self.controller.app_id,
            &self.controller.autosave_config.location,
            Some(&normalized_path),
            normalized_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&current_name),
        );

        let controller = self.controller.clone();
        let save_path = normalized_path.clone();
        let save_content = content.clone();
        let (saved_digest, ancillary_result) =
            smol::unblock(move || -> Result<(autosave::ContentDigest, Result<()>)> {
                let bytes = D::write(save_content.as_ref(), file_type)?;
                autosave::write_bytes_atomically(&save_path, &bytes)?;
                let saved_digest = autosave::content_digest(&bytes);
                let ancillary_result = (|| {
                    let _metadata_guard = controller.metadata_lock.lock();
                    controller.recent_store.record(&save_path).context(
                        "document was saved, but its recent-document entry could not be updated",
                    )?;
                    controller
                        .version_store
                        .record(&document_key(&save_path), &bytes)
                        .context(
                            "document was saved, but its version history could not be updated",
                        )?;
                    Ok(())
                })();
                Ok((saved_digest, ancillary_result))
            })
            .await?;

        let state = self.state.clone();
        let recovery_lock = self.recovery_lock.clone();
        let autosave_target = new_autosave_path.clone();
        let state_path = normalized_path.clone();
        let (dirty, dirty_changed, dirty_listeners, autosave_result) = smol::unblock(move || {
            let _recovery_guard = recovery_lock.lock();
            let mut state = state.lock();
            let dirty_before = state.dirty;
            state.file_path = Some(state_path);
            state.file_type_index = file_type_index;
            state.last_saved_snapshot = Some(content);
            state.last_saved_digest = Some(saved_digest);
            state.autosave_path = autosave_target.clone();
            state.dirty = compute_dirty(&state);

            let autosave_result: Result<()> = (|| {
                if state.dirty {
                    let autosave_bytes = D::write(state.content.as_ref(), file_type)?;
                    autosave::write_autosave(
                        &autosave_target,
                        &autosave_bytes,
                        state.last_saved_digest.as_ref(),
                    )?;
                } else {
                    autosave::clear_autosave(&autosave_target)?;
                }
                if old_autosave_path != autosave_target {
                    autosave::clear_autosave(&old_autosave_path)?;
                }
                Ok(())
            })();

            (
                state.dirty,
                dirty_before != state.dirty,
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                autosave_result,
            )
        })
        .await;

        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }

        autosave_result.context(
            "document was saved, but its autosave recovery snapshot could not be updated",
        )?;
        ancillary_result
    }

    fn restore_history_entry(&self, undo: bool) -> Result<()> {
        let (content, dirty, dirty_changed, change_listeners, dirty_listeners, autosave_result) = {
            let _recovery_guard = self.recovery_lock.lock();
            let mut state = self.state.lock();
            let next_content = if undo {
                let Some(previous) = state.undo_stack.back().cloned() else {
                    return Ok(());
                };
                previous
            } else {
                let Some(next) = state.redo_stack.back().cloned() else {
                    return Ok(());
                };
                next
            };

            let dirty = state
                .last_saved_snapshot
                .as_ref()
                .is_none_or(|saved| saved.as_ref() != next_content.as_ref());
            let current_content = state.content.clone();
            if undo {
                state.undo_stack.pop_back();
                state.redo_stack.push_back(current_content);
            } else {
                state.redo_stack.pop_back();
                state.undo_stack.push_back(current_content);
            }
            state.content = next_content;
            let previous_dirty = state.dirty;
            state.dirty = dirty;
            let autosave_result = (|| {
                if dirty {
                    let file_type = *selected_file_type::<D>(&state)?;
                    let autosave_bytes = D::write(state.content.as_ref(), &file_type)?;
                    autosave::write_autosave(
                        &state.autosave_path,
                        &autosave_bytes,
                        state.last_saved_digest.as_ref(),
                    )
                } else {
                    autosave::clear_autosave(&state.autosave_path)
                }
            })();

            (
                state.content.clone(),
                state.dirty,
                previous_dirty != state.dirty,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                autosave_result,
            )
        };

        notify_change_listeners(&change_listeners, content.as_ref());
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        autosave_result.context(
            "document history was restored, but its autosave recovery snapshot could not be updated",
        )
    }

    fn document_key(&self) -> Result<String> {
        let path = self
            .state
            .lock()
            .file_path
            .clone()
            .ok_or_else(|| anyhow!("document does not have a file path yet"))?;
        Ok(document_key(&path))
    }
}

fn selected_file_type<D: Document>(state: &DocumentState<D>) -> Result<&'static FileType> {
    D::file_types()
        .get(state.file_type_index)
        .or_else(|| D::file_types().first())
        .ok_or_else(|| {
            anyhow!(
                "document type {} does not define any file types",
                std::any::type_name::<D>()
            )
        })
}

fn compute_dirty<D: Document>(state: &DocumentState<D>) -> bool {
    state
        .last_saved_snapshot
        .as_ref()
        .map(|saved| saved.as_ref() != state.content.as_ref())
        .unwrap_or(true)
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
                            format!(
                                "failed to resolve document ancestor {}",
                                candidate.display()
                            )
                        });
                    }
                }

                let file_name = candidate.file_name().ok_or_else(|| {
                    anyhow!(
                        "failed to find an existing ancestor for document path {}",
                        absolute.display()
                    )
                })?;
                missing.push(file_name.to_os_string());
                candidate = candidate.parent().ok_or_else(|| {
                    anyhow!(
                        "failed to find an existing ancestor for document path {}",
                        absolute.display()
                    )
                })?;
            }
        }
        Err(error) => Err(error)
            .with_context(|| format!("failed to resolve document path {}", absolute.display())),
    }
}

fn read_document_bytes(path: &Path) -> Result<Vec<u8>> {
    autosave::read_regular_file_bounded(path, autosave::MAX_DOCUMENT_BYTES, "document payload")
}

fn document_key(path: &Path) -> String {
    autosave::path_digest_hex(path)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        sync::{Arc, Condvar, Mutex, OnceLock},
    };

    use futures::executor::block_on;
    use tempfile::tempdir;

    use crate::{AutosaveConfig, AutosaveLocation, FileType};

    use super::{Document, DocumentController, document_key, normalize_path};

    static CONTENT_CLONE_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct TextDocument;

    #[derive(Debug, PartialEq, Eq)]
    struct CountedContent {
        text: String,
    }

    impl Clone for CountedContent {
        fn clone(&self) -> Self {
            CONTENT_CLONE_COUNT.fetch_add(1, Ordering::SeqCst);
            Self {
                text: self.text.clone(),
            }
        }
    }

    struct CountedDocument;

    struct BlockingReadDocument;

    struct BlockingWriteDocument;

    #[derive(Default)]
    struct ReadGate {
        enabled: bool,
        started: bool,
        released: bool,
    }

    static READ_GATE: OnceLock<(Mutex<ReadGate>, Condvar)> = OnceLock::new();
    static WRITE_GATE: OnceLock<(Mutex<ReadGate>, Condvar)> = OnceLock::new();

    const TEXT_FILE_TYPES: [FileType; 1] = [FileType {
        name: "Plain Text",
        extensions: &["txt"],
        uti: Some("public.plain-text"),
        mime: Some("text/plain"),
    }];

    impl Document for TextDocument {
        type Content = String;

        fn file_types() -> &'static [FileType] {
            &TEXT_FILE_TYPES
        }

        fn new_untitled() -> Self::Content {
            String::new()
        }

        fn read(data: &[u8], _file_type: &FileType) -> crate::Result<Self::Content> {
            String::from_utf8(data.to_vec()).map_err(Into::into)
        }

        fn write(content: &Self::Content, _file_type: &FileType) -> crate::Result<Vec<u8>> {
            Ok(content.as_bytes().to_vec())
        }
    }

    impl Document for CountedDocument {
        type Content = CountedContent;

        fn file_types() -> &'static [FileType] {
            &TEXT_FILE_TYPES
        }

        fn new_untitled() -> Self::Content {
            CountedContent {
                text: String::new(),
            }
        }

        fn read(data: &[u8], _file_type: &FileType) -> crate::Result<Self::Content> {
            Ok(CountedContent {
                text: String::from_utf8(data.to_vec())?,
            })
        }

        fn write(content: &Self::Content, _file_type: &FileType) -> crate::Result<Vec<u8>> {
            Ok(content.text.as_bytes().to_vec())
        }
    }

    impl Document for BlockingReadDocument {
        type Content = String;

        fn file_types() -> &'static [FileType] {
            &TEXT_FILE_TYPES
        }

        fn new_untitled() -> Self::Content {
            String::new()
        }

        fn read(data: &[u8], _file_type: &FileType) -> crate::Result<Self::Content> {
            let (gate, condition) = READ_GATE.get_or_init(Default::default);
            let mut gate = gate.lock().unwrap();
            if gate.enabled {
                gate.started = true;
                condition.notify_all();
                while !gate.released {
                    gate = condition.wait(gate).unwrap();
                }
            }
            drop(gate);
            String::from_utf8(data.to_vec()).map_err(Into::into)
        }

        fn write(content: &Self::Content, _file_type: &FileType) -> crate::Result<Vec<u8>> {
            Ok(content.as_bytes().to_vec())
        }
    }

    impl Document for BlockingWriteDocument {
        type Content = String;

        fn file_types() -> &'static [FileType] {
            &TEXT_FILE_TYPES
        }

        fn new_untitled() -> Self::Content {
            String::new()
        }

        fn read(data: &[u8], _file_type: &FileType) -> crate::Result<Self::Content> {
            String::from_utf8(data.to_vec()).map_err(Into::into)
        }

        fn write(content: &Self::Content, _file_type: &FileType) -> crate::Result<Vec<u8>> {
            let (gate, condition) = WRITE_GATE.get_or_init(Default::default);
            let mut gate = gate.lock().unwrap();
            if gate.enabled && content == "saved" {
                gate.started = true;
                condition.notify_all();
                while !gate.released {
                    gate = condition.wait(gate).unwrap();
                }
            }
            drop(gate);
            Ok(content.as_bytes().to_vec())
        }
    }

    #[test]
    fn tracks_dirty_state_and_undo_redo() {
        let directory = tempdir().unwrap();
        let controller =
            DocumentController::<TextDocument>::new_in("dev.kael.doc.tests.undo", directory.path())
                .unwrap();
        let handle = controller.new_document();
        let dirty_states = Arc::new(Mutex::new(Vec::new()));
        let dirty_states_listener = dirty_states.clone();
        let _subscription = handle.on_dirty_change(move |dirty| {
            dirty_states_listener.lock().unwrap().push(dirty);
        });

        handle.modify(|content| content.push_str("hello")).unwrap();
        assert!(handle.is_dirty());
        assert!(handle.can_undo());
        assert_eq!(handle.content(), "hello");

        handle.undo().unwrap();
        assert_eq!(handle.content(), "");
        assert!(!handle.is_dirty());
        assert!(!handle.can_undo());
        assert!(handle.can_redo());

        handle.redo().unwrap();
        assert_eq!(handle.content(), "hello");
        assert!(handle.is_dirty());
        assert_eq!(
            dirty_states.lock().unwrap().as_slice(),
            &[true, false, true]
        );
    }

    #[test]
    fn saves_versions_and_tracks_recent_documents() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.versions",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();
        let path = directory.path().join("notes.txt");

        handle.modify(|content| content.push_str("one")).unwrap();
        block_on(handle.save_as(&path)).unwrap();
        handle
            .modify(|content| {
                content.clear();
                content.push_str("two");
            })
            .unwrap();
        block_on(handle.save()).unwrap();

        let versions = handle.versions().unwrap();
        assert_eq!(versions.len(), 2);
        let recent_documents = controller.recent_documents().unwrap();
        assert_eq!(recent_documents.len(), 1);
        assert_eq!(
            recent_documents[0].path,
            std::fs::canonicalize(path).unwrap()
        );

        block_on(handle.restore_version(&versions[0])).unwrap();
        assert_eq!(handle.content(), "one");
        assert!(handle.is_dirty());
    }

    #[test]
    fn restores_autosave_snapshots_when_reopening_documents() {
        let directory = tempdir().unwrap();
        let autosave_root = directory.path().join("autosave");
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.reopen",
            directory.path(),
        )
        .unwrap()
        .with_autosave_config(AutosaveConfig::new(AutosaveLocation::Custom(autosave_root)));
        let handle = controller.new_document();
        let path = directory.path().join("draft.txt");

        handle.modify(|content| content.push_str("saved")).unwrap();
        block_on(handle.save_as(&path)).unwrap();
        handle
            .modify(|content| {
                content.clear();
                content.push_str("autosaved");
            })
            .unwrap();
        drop(handle);

        let reopened = block_on(controller.open(&path)).unwrap();
        assert_eq!(reopened.content(), "autosaved");
        assert!(reopened.is_dirty());
    }

    #[test]
    fn stale_autosave_does_not_replace_an_external_file_revision() {
        let directory = tempdir().unwrap();
        let autosave_root = directory.path().join("autosave");
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.stale-autosave",
            directory.path(),
        )
        .unwrap()
        .with_autosave_config(AutosaveConfig::new(AutosaveLocation::Custom(autosave_root)));
        let handle = controller.new_document();
        let path = directory.path().join("draft.txt");
        handle.modify(|content| content.push_str("saved")).unwrap();
        block_on(handle.save_as(&path)).unwrap();
        handle
            .modify(|content| {
                content.clear();
                content.push_str("recovery");
            })
            .unwrap();

        std::fs::write(&path, "external").unwrap();
        let reopened = block_on(controller.open(&path)).unwrap();

        assert_eq!(reopened.content(), "external");
        assert!(!reopened.is_dirty());
    }

    #[test]
    fn corrupt_recovery_does_not_prevent_opening_the_primary_document() {
        let directory = tempdir().unwrap();
        let autosave_root = directory.path().join("autosave");
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.corrupt-autosave",
            directory.path(),
        )
        .unwrap()
        .with_autosave_config(AutosaveConfig::new(AutosaveLocation::Custom(autosave_root)));
        let handle = controller.new_document();
        let path = directory.path().join("draft.txt");
        handle.modify(|content| content.push_str("saved")).unwrap();
        block_on(handle.save_as(&path)).unwrap();
        let autosave_path = handle.state.lock().autosave_path.clone();
        std::fs::create_dir_all(autosave_path.parent().unwrap()).unwrap();
        std::fs::write(&autosave_path, b"KAEL-AUTOSAVE\0").unwrap();

        let reopened = block_on(controller.open(&path)).unwrap();

        assert_eq!(reopened.content(), "saved");
        assert!(!reopened.is_dirty());
        assert!(!autosave_path.exists());
    }

    #[test]
    fn concurrent_edits_cannot_be_overwritten_by_save_finalization() {
        let directory = tempdir().unwrap();
        let autosave_root = directory.path().join("autosave");
        let controller = DocumentController::<BlockingWriteDocument>::new_in(
            "dev.kael.doc.tests.save-autosave-race",
            directory.path(),
        )
        .unwrap()
        .with_autosave_config(AutosaveConfig::new(AutosaveLocation::Custom(autosave_root)));
        let handle = controller.new_document();
        let path = directory.path().join("draft.txt");
        handle.modify(|content| content.push_str("saved")).unwrap();

        let (gate, condition) = WRITE_GATE.get_or_init(Default::default);
        {
            let mut gate = gate.lock().unwrap();
            *gate = ReadGate {
                enabled: true,
                started: false,
                released: false,
            };
        }

        let saving_handle = handle.clone();
        let save_path = path.clone();
        let save = std::thread::spawn(move || block_on(saving_handle.save_as(save_path)));
        {
            let mut gate = gate.lock().unwrap();
            while !gate.started {
                gate = condition.wait(gate).unwrap();
            }
        }

        handle.modify(|content| content.push_str(" live")).unwrap();
        {
            let mut gate = gate.lock().unwrap();
            gate.released = true;
            gate.enabled = false;
            condition.notify_all();
        }
        save.join().unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved");
        assert_eq!(handle.content(), "saved live");
        assert!(handle.is_dirty());
        drop(handle);

        let reopened = block_on(controller.open(&path)).unwrap();
        assert_eq!(reopened.content(), "saved live");
        assert!(reopened.is_dirty());
    }

    #[test]
    fn modify_creates_one_new_content_snapshot() {
        CONTENT_CLONE_COUNT.store(0, Ordering::SeqCst);

        let directory = tempdir().unwrap();
        let controller = DocumentController::<CountedDocument>::new_in(
            "dev.kael.doc.tests.clone-count",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();

        handle
            .modify(|content| content.text.push_str("hello"))
            .unwrap();

        assert_eq!(CONTENT_CLONE_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn builder_configuration_is_not_lost_after_cloning_the_controller() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.builder-clone",
            directory.path(),
        )
        .unwrap();
        let original = controller.clone();
        let configured = controller.with_history_limit(1);

        let configured_handle = configured.new_document();
        configured_handle
            .modify(|content| content.push('1'))
            .unwrap();
        configured_handle
            .modify(|content| content.push('2'))
            .unwrap();
        configured_handle.undo().unwrap();
        assert!(!configured_handle.can_undo());

        let original_handle = original.new_document();
        original_handle.modify(|content| content.push('1')).unwrap();
        original_handle.modify(|content| content.push('2')).unwrap();
        original_handle.undo().unwrap();
        assert!(original_handle.can_undo());
    }

    #[test]
    fn independent_controllers_do_not_share_untitled_recovery_paths() {
        let first_directory = tempdir().unwrap();
        let second_directory = tempdir().unwrap();
        let first = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.untitled-identity",
            first_directory.path(),
        )
        .unwrap()
        .new_document();
        let second = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.untitled-identity",
            second_directory.path(),
        )
        .unwrap()
        .new_document();

        assert_ne!(
            first.state.lock().autosave_path,
            second.state.lock().autosave_path
        );
    }

    #[test]
    fn concurrent_edit_prevents_stale_version_restore() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<BlockingReadDocument>::new_in(
            "dev.kael.doc.tests.restore-race",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();
        let path = directory.path().join("notes.txt");
        handle.modify(|content| content.push_str("saved")).unwrap();
        block_on(handle.save_as(path)).unwrap();
        let version = handle.versions().unwrap().remove(0);

        let (gate, condition) = READ_GATE.get_or_init(Default::default);
        {
            let mut gate = gate.lock().unwrap();
            *gate = ReadGate {
                enabled: true,
                started: false,
                released: false,
            };
        }

        let restoring_handle = handle.clone();
        let restore =
            std::thread::spawn(move || block_on(restoring_handle.restore_version(&version)));
        {
            let mut gate = gate.lock().unwrap();
            while !gate.started {
                gate = condition.wait(gate).unwrap();
            }
        }

        handle
            .modify(|content| content.push_str(" live edit"))
            .unwrap();
        {
            let mut gate = gate.lock().unwrap();
            gate.released = true;
            gate.enabled = false;
            condition.notify_all();
        }

        let error = restore.join().unwrap().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("document changed while version restore was in progress")
        );
        assert_eq!(handle.content(), "saved live edit");
    }

    #[cfg(unix)]
    #[test]
    fn save_as_preserves_an_existing_symbolic_link() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.save-symlink",
            directory.path(),
        )
        .unwrap();
        let target = directory.path().join("target.txt");
        let link = directory.path().join("link.txt");
        std::fs::write(&target, b"old").unwrap();
        symlink(&target, &link).unwrap();
        let handle = controller.new_document();
        handle.modify(|content| content.push_str("new")).unwrap();

        block_on(handle.save_as(&link)).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            handle.file_path().as_deref(),
            Some(std::fs::canonicalize(target).unwrap().as_path())
        );
    }

    #[test]
    fn listener_ids_wrap_without_replacing_existing_callbacks() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.listener-wrap",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();

        let first = handle.on_change(|_| {});
        handle.state.lock().next_listener_id = usize::MAX;
        let second = handle.on_dirty_change(|_| {});
        let third = handle.on_change(|_| {});

        let state = handle.state.lock();
        assert_eq!(state.change_listeners.len(), 2);
        assert_eq!(state.dirty_listeners.len(), 1);
        assert!(state.change_listeners.contains_key(&0));
        assert!(state.dirty_listeners.contains_key(&usize::MAX));
        assert!(state.change_listeners.contains_key(&1));
        drop(state);
        drop((first, second, third));
    }

    #[test]
    fn panicking_listener_does_not_block_other_callbacks() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.listener-panic",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        let _panicking = handle.on_change(|_| panic!("listener failure"));
        let _healthy = handle.on_change(move |_| {
            calls_for_listener.fetch_add(1, Ordering::SeqCst);
        });

        handle.modify(|content| content.push_str("hello")).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(handle.content(), "hello");
    }

    #[test]
    fn listeners_can_modify_the_document_reentrantly() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.reentrant-listener",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();
        let callback_handle = handle.clone();
        let _subscription = handle.on_change(move |content| {
            if content == "first" {
                callback_handle
                    .modify(|content| content.push_str(" second"))
                    .unwrap();
            }
        });

        handle.modify(|content| content.push_str("first")).unwrap();

        assert_eq!(handle.content(), "first second");
    }

    #[test]
    fn failed_autosave_keeps_the_modification_in_memory() {
        let directory = tempdir().unwrap();
        let autosave_root = directory.path().join("autosave");
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.transactional-modify",
            directory.path(),
        )
        .unwrap()
        .with_autosave_config(AutosaveConfig::new(AutosaveLocation::Custom(autosave_root)));
        let handle = controller.new_document();
        let autosave_path = handle.state.lock().autosave_path.clone();
        std::fs::create_dir_all(&autosave_path).unwrap();

        assert!(handle.modify(|content| content.push_str("lost")).is_err());
        assert_eq!(handle.content(), "lost");
        assert!(handle.is_dirty());
        assert!(handle.can_undo());

        assert!(handle.undo().is_err());
        assert_eq!(handle.content(), "");
        assert!(!handle.is_dirty());
        assert!(handle.can_redo());
    }

    #[test]
    fn corrupt_recent_metadata_does_not_prevent_opening_a_document() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.corrupt-recent-open",
            directory.path(),
        )
        .unwrap();
        let path = directory.path().join("existing.txt");
        std::fs::write(&path, "readable").unwrap();
        std::fs::write(directory.path().join("recent_documents.json"), b"not json").unwrap();

        let handle = block_on(controller.open(&path)).unwrap();

        assert_eq!(handle.content(), "readable");
        assert!(controller.recent_documents().is_err());
    }

    #[test]
    fn save_state_tracks_successful_primary_write_when_recent_tracking_fails() {
        let directory = tempdir().unwrap();
        let controller = DocumentController::<TextDocument>::new_in(
            "dev.kael.doc.tests.partial-save",
            directory.path(),
        )
        .unwrap();
        let handle = controller.new_document();
        let path = directory.path().join("saved.txt");
        handle.modify(|content| content.push_str("saved")).unwrap();
        std::fs::write(directory.path().join("recent_documents.json"), b"not json").unwrap();

        let error = block_on(handle.save_as(&path)).unwrap_err();

        assert!(error.to_string().contains("document was saved"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved");
        assert_eq!(
            handle.file_path().as_deref(),
            Some(std::fs::canonicalize(path).unwrap().as_path())
        );
        assert!(!handle.is_dirty());
    }

    #[test]
    fn missing_document_parents_are_normalized_from_the_nearest_existing_ancestor() {
        let directory = tempdir().unwrap();
        let existing = directory.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        let requested = existing.join("missing").join("nested").join("note.txt");

        let normalized = normalize_path(&requested).unwrap();

        assert_eq!(
            normalized,
            std::fs::canonicalize(existing)
                .unwrap()
                .join("missing")
                .join("nested")
                .join("note.txt")
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_normalization_preserves_symlink_parent_semantics() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target_parent = directory.path().join("outside");
        let target = target_parent.join("target");
        let link = directory.path().join("link");
        std::fs::create_dir_all(&target).unwrap();
        symlink(&target, &link).unwrap();

        let normalized = normalize_path(&link.join("..").join("note.txt")).unwrap();

        assert_eq!(
            normalized,
            std::fs::canonicalize(target_parent)
                .unwrap()
                .join("note.txt")
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_paths_have_distinct_document_keys() {
        use std::os::unix::ffi::OsStringExt as _;

        let first = PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 0xfe]));
        let second = PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 0xff]));

        assert_ne!(document_key(&first), document_key(&second));
        assert_eq!(document_key(&first).len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn explicit_storage_roots_reject_symbolic_links() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        std::fs::create_dir(&target).unwrap();
        symlink(&target, &link).unwrap();

        assert!(
            DocumentController::<TextDocument>::new_in("dev.kael.doc.tests.storage-symlink", link,)
                .is_err()
        );
    }
}
