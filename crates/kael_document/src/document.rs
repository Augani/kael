//! Document controller and handle types.

use std::{
    collections::BTreeMap,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    AutosaveConfig, FileType, RecentDocument, autosave,
    file_type::{default_file_type_index, file_type_index_for_path},
    recent::RecentDocumentStore,
    versions::{DocumentVersion, VersionStore},
};

type ChangeListener<T> = Arc<dyn Fn(&T) + Send + Sync + 'static>;
type DirtyListener = Arc<dyn Fn(bool) + Send + Sync + 'static>;

/// A document format supported by the controller.
pub trait Document: Send + Sync + 'static {
    /// The document content model.
    type Content: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync;

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
#[derive(Clone)]
pub struct DocumentController<D: Document> {
    inner: Arc<ControllerState<D>>,
}

/// A mutable document handle managed by a controller.
#[derive(Clone)]
pub struct DocumentHandle<D: Document> {
    controller: Arc<ControllerState<D>>,
    state: Arc<Mutex<DocumentState<D>>>,
}

struct ControllerState<D: Document> {
    app_id: String,
    autosave_config: AutosaveConfig,
    recent_store: RecentDocumentStore,
    version_store: VersionStore,
    max_history_depth: usize,
    next_untitled_id: AtomicU64,
    _marker: PhantomData<fn() -> D>,
}

struct DocumentState<D: Document> {
    name: String,
    content: D::Content,
    last_saved_snapshot: Option<D::Content>,
    file_path: Option<PathBuf>,
    file_type_index: usize,
    dirty: bool,
    autosave_path: PathBuf,
    undo_stack: Vec<D::Content>,
    redo_stack: Vec<D::Content>,
    next_listener_id: usize,
    change_listeners: BTreeMap<usize, ChangeListener<D::Content>>,
    dirty_listeners: BTreeMap<usize, DirtyListener>,
}

impl<D: Document> DocumentController<D> {
    /// Creates a controller rooted in the platform-standard data directory.
    pub fn new(app_id: impl Into<String>) -> Result<Self> {
        let app_id = app_id.into();
        let root = kael_storage::platform::ensure_storage_paths(&app_id)?
            .data_dir
            .join("documents");
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

        Ok(Self {
            inner: Arc::new(ControllerState {
                app_id,
                autosave_config: AutosaveConfig::default(),
                recent_store: RecentDocumentStore::new_in(storage_root, 50)?,
                version_store: VersionStore::new_in(storage_root, 20)?,
                max_history_depth: 100,
                next_untitled_id: AtomicU64::new(1),
                _marker: PhantomData,
            }),
        })
    }

    /// Returns a controller configured with a custom autosave strategy.
    pub fn with_autosave_config(mut self, autosave_config: AutosaveConfig) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.autosave_config = autosave_config;
        }
        self
    }

    /// Returns a controller configured with a custom undo history depth.
    pub fn with_history_limit(mut self, max_history_depth: usize) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.max_history_depth = max_history_depth.max(1);
        }
        self
    }

    /// Creates a new untitled document.
    pub fn new_document(&self) -> DocumentHandle<D> {
        let untitled_id = self.inner.next_untitled_id.fetch_add(1, Ordering::Relaxed);
        let name = format!("untitled-{untitled_id}");
        let file_type_index = default_file_type_index(D::file_types()).unwrap_or(0);
        let autosave_path = autosave::autosave_path(
            &self.inner.app_id,
            &self.inner.autosave_config.location,
            None,
            &name,
        );

        DocumentHandle {
            controller: self.inner.clone(),
            state: Arc::new(Mutex::new(DocumentState {
                name,
                content: D::new_untitled(),
                last_saved_snapshot: None,
                file_path: None,
                file_type_index,
                dirty: false,
                autosave_path,
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
                next_listener_id: 0,
                change_listeners: BTreeMap::new(),
                dirty_listeners: BTreeMap::new(),
            })),
        }
    }

    /// Opens an existing document.
    pub async fn open(&self, path: impl AsRef<Path>) -> Result<DocumentHandle<D>> {
        let path = normalize_path(path.as_ref())?;
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
        let saved_bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read document from {}", path.display()))?;
        let saved_content = D::read(&saved_bytes, file_type)?;
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("document")
            .to_string();
        let autosave_path = autosave::autosave_path(
            &self.inner.app_id,
            &self.inner.autosave_config.location,
            Some(&path),
            &name,
        );

        let (content, dirty) = match autosave::load_autosave(&autosave_path)? {
            Some(autosave_bytes) => match D::read(&autosave_bytes, file_type) {
                Ok(autosave_content) if autosave_content != saved_content => {
                    (autosave_content, true)
                }
                _ => (saved_content.clone(), false),
            },
            None => (saved_content.clone(), false),
        };

        self.inner.recent_store.record(&path)?;

        Ok(DocumentHandle {
            controller: self.inner.clone(),
            state: Arc::new(Mutex::new(DocumentState {
                name,
                content,
                last_saved_snapshot: Some(saved_content),
                file_path: Some(path),
                file_type_index,
                dirty,
                autosave_path,
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
                next_listener_id: 0,
                change_listeners: BTreeMap::new(),
                dirty_listeners: BTreeMap::new(),
            })),
        })
    }

    /// Returns the recent documents tracked by this controller.
    pub fn recent_documents(&self) -> Result<Vec<RecentDocument>> {
        self.inner.recent_store.load()
    }

    /// Clears the stored recent-document list.
    pub fn clear_recent(&self) -> Result<()> {
        self.inner.recent_store.clear()
    }
}

impl<D: Document> DocumentHandle<D> {
    /// Returns a clone of the current document content.
    pub fn content(&self) -> D::Content {
        self.state.lock().content.clone()
    }

    /// Applies an in-memory change to the document content.
    pub fn modify(&self, f: impl FnOnce(&mut D::Content)) -> Result<()> {
        let (
            content,
            autosave_path,
            autosave_bytes,
            change_listeners,
            dirty_listeners,
            dirty_changed,
            dirty,
        ) = {
            let mut state = self.state.lock();
            let previous = state.content.clone();
            f(&mut state.content);

            if state.content == previous {
                return Ok(());
            }

            state.undo_stack.push(previous);
            if state.undo_stack.len() > self.controller.max_history_depth {
                state.undo_stack.remove(0);
            }
            state.redo_stack.clear();

            let file_type = selected_file_type::<D>(&state)?;
            let content = state.content.clone();
            let autosave_bytes = D::write(&content, file_type)?;
            let previous_dirty = state.dirty;
            state.dirty = compute_dirty(&state);

            (
                content,
                state.autosave_path.clone(),
                autosave_bytes,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                previous_dirty != state.dirty,
                state.dirty,
            )
        };

        autosave::write_autosave(&autosave_path, &autosave_bytes)?;
        notify_change_listeners(&change_listeners, &content);
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        Ok(())
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
        self.save_to_path(normalize_path(path.as_ref())?, true)
            .await
    }

    /// Reverts the document to the most recently saved on-disk version.
    pub async fn revert(&self) -> Result<()> {
        let (path, file_type_index, previous_dirty) = {
            let state = self.state.lock();
            (
                state
                    .file_path
                    .clone()
                    .ok_or_else(|| anyhow!("document has not been saved to a path yet"))?,
                state.file_type_index,
                state.dirty,
            )
        };

        let file_type = D::file_types()
            .get(file_type_index)
            .ok_or_else(|| anyhow!("invalid file type index {file_type_index}"))?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read document from {}", path.display()))?;
        let content = D::read(&bytes, file_type)?;

        let (change_listeners, dirty_listeners) = {
            let mut state = self.state.lock();
            state.content = content.clone();
            state.last_saved_snapshot = Some(content.clone());
            state.dirty = false;
            state.undo_stack.clear();
            state.redo_stack.clear();
            (
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };

        autosave::clear_autosave(&autosave::autosave_path(
            &self.controller.app_id,
            &self.controller.autosave_config.location,
            Some(&path),
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("document"),
        ))?;
        notify_change_listeners(&change_listeners, &content);
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
        self.controller.version_store.load(&document_key)
    }

    /// Restores a persisted version into the current document without saving it.
    pub async fn restore_version(&self, version: &DocumentVersion) -> Result<()> {
        let document_key = self.document_key()?;
        let bytes = self.controller.version_store.read(&document_key, version)?;
        let (content, autosave_path, change_listeners, dirty_listeners, dirty_changed, dirty) = {
            let mut state = self.state.lock();
            let file_type = selected_file_type::<D>(&state)?;
            let content = D::read(&bytes, file_type)?;

            if content == state.content {
                return Ok(());
            }

            let current_content = state.content.clone();
            state.undo_stack.push(current_content);
            if state.undo_stack.len() > self.controller.max_history_depth {
                state.undo_stack.remove(0);
            }
            state.redo_stack.clear();
            state.content = content.clone();
            let previous_dirty = state.dirty;
            state.dirty = compute_dirty(&state);

            (
                content,
                state.autosave_path.clone(),
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
                previous_dirty != state.dirty,
                state.dirty,
            )
        };

        if dirty {
            let file_type = {
                let state = self.state.lock();
                selected_file_type::<D>(&state)?.to_owned()
            };
            let autosave_bytes = D::write(&content, &file_type)?;
            autosave::write_autosave(&autosave_path, &autosave_bytes)?;
        } else {
            autosave::clear_autosave(&autosave_path)?;
        }

        notify_change_listeners(&change_listeners, &content);
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        Ok(())
    }

    /// Registers a listener that fires when the document content changes.
    pub fn on_change(
        &self,
        callback: impl Fn(&D::Content) + Send + Sync + 'static,
    ) -> kael_storage::Subscription {
        let state = self.state.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.next_listener_id;
            state.next_listener_id += 1;
            state
                .change_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };

        kael_storage::Subscription::new(move || {
            state.lock().change_listeners.remove(&listener_id);
        })
    }

    /// Registers a listener that fires when the dirty state changes.
    pub fn on_dirty_change(
        &self,
        callback: impl Fn(bool) + Send + Sync + 'static,
    ) -> kael_storage::Subscription {
        let state = self.state.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.next_listener_id;
            state.next_listener_id += 1;
            state
                .dirty_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };

        kael_storage::Subscription::new(move || {
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

        let (bytes, old_autosave_path, new_autosave_path, content, previous_dirty, dirty_listeners) = {
            let mut state = self.state.lock();
            let bytes = D::write(&state.content, file_type)?;
            let old_autosave_path = state.autosave_path.clone();
            let new_autosave_path = autosave::autosave_path(
                &self.controller.app_id,
                &self.controller.autosave_config.location,
                Some(&normalized_path),
                normalized_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(&state.name),
            );
            let previous_dirty = state.dirty;
            state.file_path = Some(normalized_path.clone());
            state.file_type_index = file_type_index;
            state.last_saved_snapshot = Some(state.content.clone());
            state.dirty = false;
            state.autosave_path = new_autosave_path.clone();
            (
                bytes,
                old_autosave_path,
                new_autosave_path,
                state.content.clone(),
                previous_dirty,
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };

        autosave::write_bytes_atomically(&normalized_path, &bytes)?;
        if old_autosave_path != new_autosave_path {
            autosave::clear_autosave(&old_autosave_path)?;
        }
        autosave::clear_autosave(&new_autosave_path)?;
        self.controller.recent_store.record(&normalized_path)?;
        self.controller
            .version_store
            .record(&document_key(&normalized_path), &bytes)?;

        if previous_dirty {
            notify_dirty_listeners(&dirty_listeners, false);
        }

        let _ = content;
        Ok(())
    }

    fn restore_history_entry(&self, undo: bool) -> Result<()> {
        let (
            content,
            autosave_path,
            autosave_bytes,
            dirty,
            dirty_changed,
            change_listeners,
            dirty_listeners,
        ) = {
            let mut state = self.state.lock();
            let next_content = if undo {
                let Some(previous) = state.undo_stack.pop() else {
                    return Ok(());
                };
                let current_content = state.content.clone();
                state.redo_stack.push(current_content);
                previous
            } else {
                let Some(next) = state.redo_stack.pop() else {
                    return Ok(());
                };
                let current_content = state.content.clone();
                state.undo_stack.push(current_content);
                next
            };

            state.content = next_content.clone();
            let previous_dirty = state.dirty;
            state.dirty = compute_dirty(&state);
            let autosave_bytes = if state.dirty {
                let file_type = selected_file_type::<D>(&state)?;
                Some(D::write(&state.content, file_type)?)
            } else {
                None
            };

            (
                state.content.clone(),
                state.autosave_path.clone(),
                autosave_bytes,
                state.dirty,
                previous_dirty != state.dirty,
                state.change_listeners.values().cloned().collect::<Vec<_>>(),
                state.dirty_listeners.values().cloned().collect::<Vec<_>>(),
            )
        };

        if let Some(autosave_bytes) = autosave_bytes {
            autosave::write_autosave(&autosave_path, &autosave_bytes)?;
        } else {
            autosave::clear_autosave(&autosave_path)?;
        }

        notify_change_listeners(&change_listeners, &content);
        if dirty_changed {
            notify_dirty_listeners(&dirty_listeners, dirty);
        }
        Ok(())
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
        .map(|saved| saved != &state.content)
        .unwrap_or(true)
}

fn notify_change_listeners<T>(listeners: &[ChangeListener<T>], content: &T) {
    for listener in listeners {
        listener(content);
    }
}

fn notify_dirty_listeners(listeners: &[DirtyListener], dirty: bool) {
    for listener in listeners {
        listener(dirty);
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(std::env::current_dir()
        .context("failed to resolve current working directory")?
        .join(path))
}

fn document_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use futures::executor::block_on;
    use tempfile::tempdir;

    use crate::{AutosaveConfig, AutosaveLocation, FileType};

    use super::{Document, DocumentController};

    struct TextDocument;

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

    #[test]
    fn tracks_dirty_state_and_undo_redo() {
        let directory = tempdir().unwrap();
        let controller =
            DocumentController::<TextDocument>::new_in("dev.kael.doc.tests", directory.path())
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
        assert!(!handle.can_undo());
        assert!(handle.can_redo());

        handle.redo().unwrap();
        assert_eq!(handle.content(), "hello");
        assert_eq!(dirty_states.lock().unwrap().as_slice(), &[true]);
    }

    #[test]
    fn saves_versions_and_tracks_recent_documents() {
        let directory = tempdir().unwrap();
        let controller =
            DocumentController::<TextDocument>::new_in("dev.kael.doc.tests", directory.path())
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
        assert_eq!(recent_documents[0].path, path);

        block_on(handle.restore_version(&versions[0])).unwrap();
        assert_eq!(handle.content(), "one");
        assert!(handle.is_dirty());
    }

    #[test]
    fn restores_autosave_snapshots_when_reopening_documents() {
        let directory = tempdir().unwrap();
        let autosave_root = directory.path().join("autosave");
        let controller =
            DocumentController::<TextDocument>::new_in("dev.kael.doc.tests", directory.path())
                .unwrap()
                .with_autosave_config(AutosaveConfig {
                    interval: std::time::Duration::from_secs(30),
                    location: AutosaveLocation::Custom(autosave_root),
                });
        let handle = controller.new_document();
        let path = PathBuf::from(directory.path().join("draft.txt"));

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
}
