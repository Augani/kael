#[cfg(not(target_arch = "wasm32"))]
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

#[cfg(not(target_arch = "wasm32"))]
use anyhow::{Context as _, anyhow};
use anyhow::{Result, bail};
#[cfg(not(target_arch = "wasm32"))]
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
#[cfg(not(target_arch = "wasm32"))]
use smol::channel;

#[cfg(not(target_arch = "wasm32"))]
use crate::Task;
use crate::{App, ForegroundExecutor};

const MAX_WATCH_REGISTRATIONS: usize = 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_QUEUED_WATCH_EVENTS: usize = 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PATHS_PER_NOTIFY_EVENT: usize = 256;

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

    /// Whether a recursive depth limit is configured.
    pub fn has_depth_limit(&self) -> bool {
        self.max_depth.is_some()
    }

    /// Content-safe summary for logs and generated watch plans.
    pub fn to_text(&self) -> String {
        format!(
            "file watch options: recursive {}, depth-limit {}",
            self.recursive,
            self.has_depth_limit()
        )
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

    /// Content-safe summary for generated watch option builders.
    pub fn to_text(&self) -> String {
        self.options.to_text()
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

    /// Number of canonicalized paths in this set.
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// Whether this set watches more than one path.
    pub fn has_multiple_paths(&self) -> bool {
        self.path_count() > 1
    }

    /// Content-safe summary that avoids logging watched paths.
    pub fn to_text(&self) -> String {
        format!(
            "file watch set: paths {}, multiple {}, recursive {}, depth-limit {}",
            self.path_count(),
            self.has_multiple_paths(),
            self.options.recursive,
            self.options.has_depth_limit()
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
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

    /// Number of raw configured paths.
    pub fn configured_path_count(&self) -> usize {
        self.paths.len()
    }

    /// Whether this builder currently has multiple raw configured paths.
    pub fn has_multiple_paths(&self) -> bool {
        self.configured_path_count() > 1
    }

    /// Content-safe summary that avoids logging watched paths.
    pub fn to_text(&self) -> String {
        format!(
            "file watch set builder: paths {}, multiple {}, recursive {}, depth-limit {}",
            self.configured_path_count(),
            self.has_multiple_paths(),
            self.options.recursive,
            self.options.has_depth_limit()
        )
    }

    /// Validate the grouped watch request without resolving paths.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.paths.is_empty(),
            "at least one file watch path must be configured"
        );
        anyhow::ensure!(
            self.paths.len() <= MAX_WATCH_REGISTRATIONS,
            "file watch set cannot contain more than {MAX_WATCH_REGISTRATIONS} paths"
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

#[cfg(not(target_arch = "wasm32"))]
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

impl FileWatchEvent {
    /// Stable event kind key for routing and diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Created(_) => "created",
            Self::Modified(_) => "modified",
            Self::Deleted(_) => "deleted",
            Self::Renamed { .. } => "renamed",
            Self::Error { .. } => "error",
        }
    }

    /// Whether this event carries a path that no longer exists.
    pub fn is_removal(&self) -> bool {
        matches!(self, Self::Deleted(_) | Self::Renamed { .. })
    }

    /// Whether this event is an error event.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Content-safe summary that avoids logging paths or error text.
    pub fn to_text(&self) -> String {
        format!(
            "file watch event: kind {}, removal {}, error {}",
            self.kind(),
            self.is_removal(),
            self.is_error()
        )
    }
}

/// Cross-platform file-system watcher backed by the `notify` crate.
///
/// Callbacks are always executed on the GPUI foreground executor so they can
/// safely interact with other UI state.
#[cfg(not(target_arch = "wasm32"))]
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    registrations: Arc<Mutex<BTreeMap<PathBuf, WatchRegistration>>>,
    event_tx: channel::Sender<FileWatchEvent>,
    _callback_task: Task<()>,
}

#[cfg(not(target_arch = "wasm32"))]
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
        let (event_tx, event_rx) = channel::bounded(MAX_QUEUED_WATCH_EVENTS);
        let callback_task = executor.spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                dispatch_watch_callback(&mut callback, event);
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

        let registration = WatchRegistration {
            recursive: options.recursive,
            max_depth: options.max_depth,
        };
        {
            let mut registrations = self
                .registrations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            anyhow::ensure!(
                !registrations.contains_key(&normalized_path),
                "file watch path is already registered: {}",
                normalized_path.display()
            );
            anyhow::ensure!(
                registrations.len() < MAX_WATCH_REGISTRATIONS,
                "file watcher cannot contain more than {MAX_WATCH_REGISTRATIONS} registrations"
            );
            registrations.insert(normalized_path.clone(), registration);
        }

        if let Err(error) = self
            .watcher
            .watch(&normalized_path, recursive_mode)
            .with_context(|| format!("failed to watch {}", normalized_path.display()))
        {
            self.registrations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&normalized_path);
            return Err(error);
        }

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
        validate_watch_input_path(path.as_ref())?;
        let resolved_path = resolve_input_path(path.as_ref())?;
        let normalized_path = if resolved_path.exists() {
            resolved_path
                .canonicalize()
                .with_context(|| format!("failed to canonicalize {}", resolved_path.display()))?
        } else {
            resolved_path
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

/// Browser placeholder for Kael's file-system watcher.
///
/// Browsers cannot observe arbitrary local paths. This implementation keeps
/// configuration and application startup portable while explicit browser file
/// picker interactions remain the refresh boundary.
#[cfg(target_arch = "wasm32")]
pub struct FileWatcher;

#[cfg(target_arch = "wasm32")]
impl FileWatcher {
    /// Create a browser file-watcher placeholder.
    pub fn new(_app: &App, _callback: impl FnMut(FileWatchEvent) + 'static) -> Result<Self> {
        Ok(Self)
    }

    /// Create a browser file-watcher placeholder.
    pub fn new_with_executor(
        _executor: ForegroundExecutor,
        _callback: impl FnMut(FileWatchEvent) + 'static,
    ) -> Result<Self> {
        Ok(Self)
    }

    /// Validate a watch request. Browser builds do not emit path events.
    pub fn watch(&mut self, path: impl AsRef<Path>, recursive: bool) -> Result<()> {
        self.watch_with_options(
            path,
            FileWatchOptions {
                recursive,
                max_depth: None,
            },
        )
    }

    /// Validate a watch request. Browser builds do not emit path events.
    pub fn watch_with_options(
        &mut self,
        _path: impl AsRef<Path>,
        options: FileWatchOptions,
    ) -> Result<()> {
        options.validate()
    }

    /// Validate and return a browser watch-set description.
    pub fn watch_set(&mut self, watch_set: impl Into<FileWatchSetBuilder>) -> Result<FileWatchSet> {
        watch_set.into().build_checked()
    }

    /// Browser placeholders have no native registration to remove.
    pub fn unwatch(&mut self, _path: impl AsRef<Path>) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn dispatch_watch_callback(callback: &mut impl FnMut(FileWatchEvent), event: FileWatchEvent) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event)));
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
fn normalize_watch_path(path: &Path) -> Result<PathBuf> {
    validate_watch_input_path(path)?;
    Ok(path.to_path_buf())
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn translate_notify_result(
    result: notify::Result<Event>,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> Vec<FileWatchEvent> {
    match result {
        Ok(event) => translate_notify_event(event, registrations),
        Err(error) => translate_notify_error(error, registrations),
    }
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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
        .take(MAX_PATHS_PER_NOTIFY_EVENT)
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

#[cfg(not(target_arch = "wasm32"))]
fn paths_matching_registrations(
    paths: &[PathBuf],
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| path_matches_any_registration(path, registrations))
        .take(MAX_PATHS_PER_NOTIFY_EVENT)
        .cloned()
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn rename_matches_registrations(
    from: &Path,
    to: &Path,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> bool {
    path_matches_any_registration(from, registrations)
        || path_matches_any_registration(to, registrations)
}

#[cfg(not(target_arch = "wasm32"))]
fn path_matches_any_registration(
    path: &Path,
    registrations: &BTreeMap<PathBuf, WatchRegistration>,
) -> bool {
    registrations
        .iter()
        .any(|(root, registration)| path_matches_registration(path, root, registration))
}

#[cfg(not(target_arch = "wasm32"))]
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
        assert!(!default_options.has_depth_limit());
        assert_eq!(
            default_options.to_text(),
            "file watch options: recursive false, depth-limit false"
        );
        assert_eq!(
            FileWatchOptionsBuilder::new().to_text(),
            "file watch options: recursive false, depth-limit false"
        );

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
        assert!(depth_limited.has_depth_limit());
        assert_eq!(
            depth_limited.to_text(),
            "file watch options: recursive true, depth-limit true"
        );
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

        let builder_summary = FileWatchSetBuilder::new()
            .path(project.clone())
            .path(config.clone())
            .max_depth(2)
            .to_text();
        assert_eq!(
            builder_summary,
            "file watch set builder: paths 2, multiple true, recursive true, depth-limit true"
        );
        assert!(!builder_summary.contains("project"));
        assert!(!builder_summary.contains("config.toml"));

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
        assert_eq!(watch_set.path_count(), 2);
        assert!(watch_set.has_multiple_paths());
        assert_eq!(
            watch_set.to_text(),
            "file watch set: paths 2, multiple true, recursive true, depth-limit true"
        );
        assert!(!watch_set.to_text().contains("project"));
        assert!(!watch_set.to_text().contains("config.toml"));

        assert!(FileWatchSetBuilder::new().validate().is_err());
        assert!(FileWatchSetBuilder::new().path("").validate().is_err());
        assert!(
            FileWatchSetBuilder::new()
                .paths((0..=MAX_WATCH_REGISTRATIONS).map(|index| format!("path-{index}")))
                .validate()
                .is_err()
        );
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
    fn file_watch_event_summaries_do_not_leak_paths_or_errors() {
        let created = FileWatchEvent::Created(PathBuf::from("/private/project/src/main.rs"));
        assert_eq!(created.kind(), "created");
        assert!(!created.is_removal());
        assert!(!created.is_error());
        assert_eq!(
            created.to_text(),
            "file watch event: kind created, removal false, error false"
        );
        assert!(!created.to_text().contains("private"));

        let renamed = FileWatchEvent::Renamed {
            from: PathBuf::from("/private/project/old.rs"),
            to: PathBuf::from("/private/project/new.rs"),
        };
        assert_eq!(renamed.kind(), "renamed");
        assert!(renamed.is_removal());
        assert!(!renamed.to_text().contains("old.rs"));
        assert!(!renamed.to_text().contains("new.rs"));

        let error = FileWatchEvent::Error {
            path: PathBuf::from("/private/project"),
            error: io::Error::other("permission denied for /private/project"),
        };
        assert_eq!(error.kind(), "error");
        assert!(error.is_error());
        assert_eq!(
            error.to_text(),
            "file watch event: kind error, removal false, error true"
        );
        assert!(!error.to_text().contains("permission denied"));
        assert!(!error.to_text().contains("private"));
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

    #[test]
    fn translated_event_paths_are_bounded() {
        let root = PathBuf::from("/tmp/root");
        let mut registrations = BTreeMap::new();
        registrations.insert(
            root.clone(),
            WatchRegistration {
                recursive: true,
                max_depth: None,
            },
        );
        let paths = (0..(MAX_PATHS_PER_NOTIFY_EVENT + 10))
            .map(|index| root.join(format!("file-{index}")))
            .collect::<Vec<_>>();

        let translated = paths_matching_registrations(&paths, &registrations);
        assert_eq!(translated.len(), MAX_PATHS_PER_NOTIFY_EVENT);
    }

    #[test]
    fn panicking_watch_callbacks_do_not_block_later_events() {
        let mut calls = 0;
        let mut callback = |_event| {
            calls += 1;
            assert_ne!(calls, 1, "first callback panic");
        };

        dispatch_watch_callback(
            &mut callback,
            FileWatchEvent::Created(PathBuf::from("first")),
        );
        dispatch_watch_callback(
            &mut callback,
            FileWatchEvent::Created(PathBuf::from("second")),
        );
        assert_eq!(calls, 2);
    }
}
