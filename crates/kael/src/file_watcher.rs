use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use smol::channel;

use crate::{App, ForegroundExecutor, Task};

/// Options that control how a path is watched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileWatchOptions {
    /// Whether directory changes should be watched recursively.
    pub recursive: bool,
    /// The maximum relative depth to emit events for when `recursive` is enabled.
    ///
    /// A depth of `1` includes direct children of the watched directory, `2`
    /// includes grandchildren, and so on. `None` means there is no depth limit.
    pub max_depth: Option<usize>,
}

impl FileWatchOptions {
    /// Returns recursive watch options with no depth limit.
    pub fn recursive() -> Self {
        Self {
            recursive: true,
            max_depth: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WatchRegistration {
    recursive: bool,
    max_depth: Option<usize>,
}

/// A file-system change delivered by [`FileWatcher`].
#[derive(Debug)]
pub enum FileWatchEvent {
    /// A new file or directory was created.
    Created(PathBuf),
    /// An existing file or directory was modified.
    Modified(PathBuf),
    /// A file or directory was deleted.
    Deleted(PathBuf),
    /// A file or directory was renamed or moved.
    Renamed {
        /// The previous path.
        from: PathBuf,
        /// The new path.
        to: PathBuf,
    },
    /// Watching failed for a specific path.
    Error {
        /// The watched path associated with the error.
        path: PathBuf,
        /// The underlying I/O error.
        error: io::Error,
    },
}

/// Cross-platform file-system watcher backed by the `notify` crate.
///
/// Callbacks are always executed on the GPUI foreground executor so they can
/// safely interact with other UI state.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    registrations: Arc<Mutex<BTreeMap<PathBuf, WatchRegistration>>>,
    event_tx: channel::Sender<FileWatchEvent>,
    _callback_task: Task<()>,
}

impl FileWatcher {
    /// Creates a file watcher that dispatches callbacks on the given app's
    /// foreground executor.
    pub fn new(app: &App, callback: impl FnMut(FileWatchEvent) + 'static) -> Result<Self> {
        Self::new_with_executor(app.foreground_executor().clone(), callback)
    }

    /// Creates a file watcher that dispatches callbacks on the given
    /// foreground executor.
    pub fn new_with_executor(
        executor: ForegroundExecutor,
        mut callback: impl FnMut(FileWatchEvent) + 'static,
    ) -> Result<Self> {
        let registrations = Arc::new(Mutex::new(BTreeMap::new()));
        let (event_tx, event_rx) = channel::unbounded();
        let callback_task = executor.spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                callback(event);
            }
        });

        let watcher_registrations = registrations.clone();
        let watcher_tx = event_tx.clone();
        let mut watcher = notify::recommended_watcher(move |result| {
            let events = {
                let registrations = watcher_registrations
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                translate_notify_result(result, &registrations)
            };

            for event in events {
                let _ = watcher_tx.try_send(event);
            }
        })
        .context("failed to create file watcher")?;

        watcher
            .configure(Config::default())
            .context("failed to configure file watcher")?;

        Ok(Self {
            watcher,
            registrations,
            event_tx,
            _callback_task: callback_task,
        })
    }

    /// Starts watching a file or directory.
    ///
    /// When `recursive` is `true`, all descendants are watched without a depth
    /// limit. Use [`Self::watch_with_options`] to constrain recursive depth.
    pub fn watch(&mut self, path: impl AsRef<Path>, recursive: bool) -> Result<()> {
        self.watch_with_options(
            path,
            FileWatchOptions {
                recursive,
                max_depth: None,
            },
        )
    }

    /// Starts watching a file or directory with explicit options.
    pub fn watch_with_options(
        &mut self,
        path: impl AsRef<Path>,
        options: FileWatchOptions,
    ) -> Result<()> {
        if options.max_depth.is_some() && !options.recursive {
            bail!("file watch depth limits require recursive watching");
        }

        let normalized_path = match normalize_watch_path(path.as_ref()) {
            Ok(path) => path,
            Err(error) => {
                self.emit_watch_error(resolve_input_path(path.as_ref())?, anyhow!("{error}"));
                return Err(error);
            }
        };
        let recursive_mode = if options.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        self.watcher
            .watch(&normalized_path, recursive_mode)
            .with_context(|| format!("failed to watch {}", normalized_path.display()))?;

        let registration = WatchRegistration {
            recursive: options.recursive,
            max_depth: options.max_depth,
        };
        self.registrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(normalized_path, registration);

        Ok(())
    }

    /// Stops watching a previously-registered path.
    pub fn unwatch(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let normalized_path = match normalize_watch_path(path.as_ref()) {
            Ok(path) => path,
            Err(error) => {
                self.emit_watch_error(resolve_input_path(path.as_ref())?, anyhow!("{error}"));
                return Err(error);
            }
        };
        self.watcher
            .unwatch(&normalized_path)
            .with_context(|| format!("failed to unwatch {}", normalized_path.display()))?;
        self.registrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&normalized_path);
        Ok(())
    }

    fn emit_watch_error(&self, path: PathBuf, error: anyhow::Error) {
        let _ = self.event_tx.try_send(FileWatchEvent::Error {
            path,
            error: io::Error::other(error.to_string()),
        });
    }
}

fn normalize_watch_path(path: &Path) -> Result<PathBuf> {
    let absolute = resolve_input_path(path)?;

    if !absolute.exists() {
        bail!("watched path does not exist: {}", absolute.display());
    }

    absolute
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", absolute.display()))
}

fn resolve_input_path(path: &Path) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current working directory")?
            .join(path)
    })
}

fn translate_notify_result(
    result: notify::Result<Event>,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> Vec<FileWatchEvent> {
    match result {
        Ok(event) => translate_notify_event(event, registrations),
        Err(error) => translate_notify_error(error, registrations),
    }
}

fn translate_notify_event(
    event: Event,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> Vec<FileWatchEvent> {
    if matches!(event.kind, EventKind::Access(_)) {
        return Vec::new();
    }

    match event.kind {
        EventKind::Create(
            CreateKind::Any | CreateKind::File | CreateKind::Folder | CreateKind::Other,
        ) => paths_matching_registrations(&event.paths, registrations)
            .into_iter()
            .map(FileWatchEvent::Created)
            .collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Any | RenameMode::Both))
            if event.paths.len() >= 2
                && rename_matches_registrations(
                    &event.paths[0],
                    &event.paths[1],
                    registrations,
                ) =>
        {
            vec![FileWatchEvent::Renamed {
                from: event.paths[0].clone(),
                to: event.paths[1].clone(),
            }]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            paths_matching_registrations(&event.paths, registrations)
                .into_iter()
                .map(FileWatchEvent::Deleted)
                .collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            paths_matching_registrations(&event.paths, registrations)
                .into_iter()
                .map(FileWatchEvent::Created)
                .collect()
        }
        EventKind::Modify(_) => paths_matching_registrations(&event.paths, registrations)
            .into_iter()
            .map(FileWatchEvent::Modified)
            .collect(),
        EventKind::Remove(
            RemoveKind::Any | RemoveKind::File | RemoveKind::Folder | RemoveKind::Other,
        ) => paths_matching_registrations(&event.paths, registrations)
            .into_iter()
            .map(FileWatchEvent::Deleted)
            .collect(),
        _ => Vec::new(),
    }
}

fn translate_notify_error(
    error: notify::Error,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> Vec<FileWatchEvent> {
    let message = error.to_string();
    let mut paths = error.paths;
    if paths.is_empty() {
        paths.extend(registrations.keys().cloned());
    }
    paths
        .into_iter()
        .filter(|path| {
            registrations.is_empty()
                || registrations.contains_key(path)
                || path_matches_any_registration(path, registrations)
        })
        .map(|path| FileWatchEvent::Error {
            path,
            error: io::Error::other(message.clone()),
        })
        .collect()
}

fn paths_matching_registrations(
    paths: &[PathBuf],
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| path_matches_any_registration(path, registrations))
        .cloned()
        .collect()
}

fn rename_matches_registrations(
    from: &Path,
    to: &Path,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> bool {
    path_matches_any_registration(from, registrations)
        || path_matches_any_registration(to, registrations)
}

fn path_matches_any_registration(
    path: &Path,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> bool {
    registrations
        .iter()
        .any(|(root, registration)| path_matches_registration(path, root, registration))
}

fn path_matches_registration(path: &Path, root: &Path, registration: &WatchRegistration) -> bool {
    if path == root {
        return true;
    }

    let Ok(relative_path) = path.strip_prefix(root) else {
        return false;
    };

    let depth = relative_path.components().count();
    if depth == 0 {
        return true;
    }

    if !registration.recursive {
        return depth == 1;
    }

    registration
        .max_depth
        .is_none_or(|max_depth| depth <= max_depth)
}

#[cfg(any(test, feature = "test-support"))]
#[allow(dead_code)]
pub(crate) fn translate_watch_event_for_test(
    result: notify::Result<Event>,
    watched_path: &Path,
    options: FileWatchOptions,
) -> Vec<FileWatchEvent> {
    let mut registrations = BTreeMap::new();
    registrations.insert(
        watched_path.to_path_buf(),
        WatchRegistration {
            recursive: options.recursive,
            max_depth: options.max_depth,
        },
    );
    translate_notify_result(result, &registrations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_matching_honors_depth_limits() {
        let root = PathBuf::from("/tmp/root");
        let registration = WatchRegistration {
            recursive: true,
            max_depth: Some(2),
        };

        assert!(path_matches_registration(
            Path::new("/tmp/root/child"),
            &root,
            &registration
        ));
        assert!(path_matches_registration(
            Path::new("/tmp/root/child/grandchild"),
            &root,
            &registration
        ));
        assert!(!path_matches_registration(
            Path::new("/tmp/root/child/grandchild/great-grandchild"),
            &root,
            &registration
        ));
    }

    #[test]
    fn path_matching_honors_non_recursive_watches() {
        let root = PathBuf::from("/tmp/root");
        let registration = WatchRegistration {
            recursive: false,
            max_depth: None,
        };

        assert!(path_matches_registration(
            Path::new("/tmp/root/file.txt"),
            &root,
            &registration
        ));
        assert!(!path_matches_registration(
            Path::new("/tmp/root/nested/file.txt"),
            &root,
            &registration
        ));
    }
}
