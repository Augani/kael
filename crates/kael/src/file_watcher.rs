use std::{
    collections::{BTreeMap, BTreeSet},
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
    /// Returns non-recursive watch options.
    pub fn non_recursive() -> Self {
        Self {
            recursive: false,
            max_depth: None,
        }
    }

    /// Returns recursive watch options with no depth limit.
    pub fn recursive() -> Self {
        Self {
            recursive: true,
            max_depth: None,
        }
    }

    /// Returns recursive watch options with a maximum relative depth.
    pub fn recursive_depth(max_depth: usize) -> Self {
        Self {
            recursive: true,
            max_depth: Some(max_depth),
        }
    }

    /// Validate the watch options before registering a path.
    pub fn validate(&self) -> Result<()> {
        if self.max_depth.is_some() && !self.recursive {
            bail!("file watch depth limits require recursive watching");
        }
        if let Some(max_depth) = self.max_depth {
            anyhow::ensure!(
                max_depth > 0,
                "file watch max depth must be greater than zero"
            );
        }
        Ok(())
    }
}

/// Builder for file-system watch options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileWatchOptionsBuilder {
    options: FileWatchOptions,
}

impl FileWatchOptionsBuilder {
    /// Create non-recursive watch options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Watch only the configured path and direct child events.
    pub fn non_recursive(mut self) -> Self {
        self.options.recursive = false;
        self.options.max_depth = None;
        self
    }

    /// Watch descendants recursively without a depth limit.
    pub fn recursive(mut self) -> Self {
        self.options.recursive = true;
        self.options.max_depth = None;
        self
    }

    /// Watch descendants recursively up to a maximum relative depth.
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.options.recursive = true;
        self.options.max_depth = Some(max_depth);
        self
    }

    /// Return the configured options.
    pub fn options(&self) -> &FileWatchOptions {
        &self.options
    }

    /// Validate the configured options.
    pub fn validate(&self) -> Result<()> {
        self.options.validate()
    }

    /// Build the validated file watch options.
    pub fn build_checked(self) -> Result<FileWatchOptions> {
        self.validate()?;
        Ok(self.options)
    }
}

/// A validated group of file-system paths to watch with shared options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileWatchSet {
    paths: Vec<PathBuf>,
    options: FileWatchOptions,
}

impl FileWatchSet {
    /// The canonicalized paths that should be registered with the watcher.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The watch options shared by every path in this set.
    pub fn options(&self) -> &FileWatchOptions {
        &self.options
    }

    fn into_parts(self) -> (Vec<PathBuf>, FileWatchOptions) {
        (self.paths, self.options)
    }
}

/// Builder for registering multiple file-system watch roots together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileWatchSetBuilder {
    paths: Vec<PathBuf>,
    options: FileWatchOptions,
}

impl FileWatchSetBuilder {
    /// Create an empty grouped watch request with non-recursive options.
    pub fn new() -> Self {
        Self {
            paths: Vec::new(),
            options: FileWatchOptions::non_recursive(),
        }
    }

    /// Add one path to the watch set.
    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.paths.push(path.into());
        self
    }

    /// Add multiple paths to the watch set.
    pub fn paths(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        for path in paths {
            self = self.path(path);
        }
        self
    }

    /// Use explicit watch options for every path in the set.
    pub fn options(mut self, options: FileWatchOptions) -> Self {
        self.options = options;
        self
    }

    /// Watch only each configured path and direct child events.
    pub fn non_recursive(mut self) -> Self {
        self.options = FileWatchOptions::non_recursive();
        self
    }

    /// Watch descendants recursively without a depth limit.
    pub fn recursive(mut self) -> Self {
        self.options = FileWatchOptions::recursive();
        self
    }

    /// Watch descendants recursively up to a maximum relative depth.
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.options = FileWatchOptions::recursive_depth(max_depth);
        self
    }

    /// Return the raw configured paths.
    pub fn configured_paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Return the configured shared options.
    pub fn configured_options(&self) -> &FileWatchOptions {
        &self.options
    }

    /// Validate the grouped watch request without resolving paths.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.paths.is_empty(),
            "at least one file watch path must be configured"
        );
        self.options.validate()?;
        for path in &self.paths {
            validate_watch_input_path(path)?;
        }
        Ok(())
    }

    /// Build a validated set with canonicalized, deduplicated paths.
    pub fn build_checked(self) -> Result<FileWatchSet> {
        self.validate()?;

        let mut seen = BTreeSet::new();
        let mut paths = Vec::new();
        for path in self.paths {
            let normalized = normalize_watch_path(&path)?;
            if seen.insert(normalized.clone()) {
                paths.push(normalized);
            }
        }

        Ok(FileWatchSet {
            paths,
            options: self.options,
        })
    }
}

impl Default for FileWatchSetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl From<PathBuf> for FileWatchSetBuilder {
    fn from(path: PathBuf) -> Self {
        Self::new().path(path)
    }
}

impl From<&Path> for FileWatchSetBuilder {
    fn from(path: &Path) -> Self {
        Self::new().path(path)
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
        options.validate()?;

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

    /// Starts watching multiple paths with shared, validated options.
    pub fn watch_set(&mut self, watch_set: impl Into<FileWatchSetBuilder>) -> Result<FileWatchSet> {
        let watch_set = watch_set.into().build_checked()?;
        let (paths, options) = watch_set.clone().into_parts();
        let mut registered_paths = Vec::new();

        for path in paths {
            if let Err(error) = self.watch_with_options(&path, options.clone()) {
                for registered_path in registered_paths {
                    let _ = self.unwatch(registered_path);
                }
                return Err(error);
            }
            registered_paths.push(path);
        }

        Ok(watch_set)
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
    validate_watch_input_path(path)?;
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

fn validate_watch_input_path(path: &Path) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "file watch path cannot be empty"
    );
    anyhow::ensure!(
        !path.to_string_lossy().contains('\0'),
        "file watch path cannot contain NUL bytes"
    );
    Ok(())
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
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_watch_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kael-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn file_watch_options_builder_validates_depth_policy() {
        let default_options = FileWatchOptionsBuilder::new().build_checked().unwrap();
        assert!(!default_options.recursive);
        assert_eq!(default_options.max_depth, None);

        let recursive = FileWatchOptionsBuilder::new()
            .recursive()
            .build_checked()
            .unwrap();
        assert!(recursive.recursive);
        assert_eq!(recursive.max_depth, None);

        let depth_limited = FileWatchOptionsBuilder::new()
            .max_depth(2)
            .build_checked()
            .unwrap();
        assert!(depth_limited.recursive);
        assert_eq!(depth_limited.max_depth, Some(2));
        assert_eq!(
            FileWatchOptions::recursive_depth(2),
            FileWatchOptions {
                recursive: true,
                max_depth: Some(2)
            }
        );

        assert!(FileWatchOptions::non_recursive().validate().is_ok());
        assert!(FileWatchOptions::recursive().validate().is_ok());
        assert!(FileWatchOptions::recursive_depth(1).validate().is_ok());
        assert!(
            FileWatchOptions {
                recursive: false,
                max_depth: Some(1),
            }
            .validate()
            .is_err()
        );
        assert!(
            FileWatchOptions {
                recursive: true,
                max_depth: Some(0),
            }
            .validate()
            .is_err()
        );
        assert!(
            FileWatchOptionsBuilder::new()
                .max_depth(0)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn file_watch_set_builder_validates_and_canonicalizes_paths() {
        let root = unique_watch_test_dir("watch-set");
        let project = root.join("project");
        let config = root.join("config.toml");
        fs::create_dir_all(&project).unwrap();
        fs::write(&config, "theme = 'dark'\n").unwrap();

        let watch_set = FileWatchSetBuilder::new()
            .path(project.clone())
            .path(project.clone())
            .path(config.clone())
            .max_depth(2)
            .build_checked()
            .unwrap();

        assert_eq!(
            watch_set.paths(),
            &[
                project.canonicalize().unwrap(),
                config.canonicalize().unwrap()
            ]
        );
        assert_eq!(
            watch_set.options(),
            &FileWatchOptions {
                recursive: true,
                max_depth: Some(2)
            }
        );

        assert!(FileWatchSetBuilder::new().validate().is_err());
        assert!(FileWatchSetBuilder::new().path("").validate().is_err());
        assert!(
            FileWatchSetBuilder::new()
                .path(root.join("missing"))
                .build_checked()
                .is_err()
        );
        assert!(
            FileWatchSetBuilder::new()
                .path(project.clone())
                .options(FileWatchOptions {
                    recursive: false,
                    max_depth: Some(1),
                })
                .validate()
                .is_err()
        );

        let _ = fs::remove_dir_all(root);
    }

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
