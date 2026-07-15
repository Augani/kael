use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::{DisplayId, WindowBounds, WindowState};

const MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SESSION_WINDOWS: usize = 1024;

/// Persistent storage for application session state.
///
/// Stores window geometry and display associations across launches, restoring
/// windows to their previous positions when possible and falling back to the
/// primary display when the previously-used display is no longer available.
#[derive(Debug)]
pub struct SessionStore {
    app_id: String,
    storage_dir: PathBuf,
}

impl SessionStore {
    /// Create a new session store for the given application identifier.
    ///
    /// The `app_id` is used to derive a platform-appropriate storage directory.
    pub fn new(app_id: impl Into<String>) -> Result<Self> {
        let app_id = app_id.into();
        validate_session_app_id(&app_id)?;
        let storage_dir = session_storage_dir(&app_id)?;
        std::fs::create_dir_all(&storage_dir).with_context(|| {
            format!(
                "failed to create session storage directory: {}",
                storage_dir.display()
            )
        })?;
        validate_session_directory(&storage_dir)?;
        restrict_session_directory_permissions(&storage_dir)?;

        Ok(Self {
            app_id,
            storage_dir,
        })
    }

    /// Create a new session store after validating the application identifier.
    pub fn new_checked(app_id: impl Into<String>) -> Result<Self> {
        let app_id = app_id.into();
        validate_session_app_id(&app_id)?;
        Self::new(app_id)
    }

    /// Returns the application identifier associated with this store.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Returns the directory where session data is persisted.
    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
    }

    /// Save the current window states to persistent storage.
    ///
    /// Window states are keyed by an arbitrary identifier chosen by the
    /// application (e.g., `"main"`, `"settings"`).
    pub fn save_window_states(&self, states: &HashMap<String, WindowState>) -> Result<()> {
        let existing = self.load_snapshot()?;
        let snapshot = SessionSnapshot {
            window_states: states.clone(),
            app_data: existing.app_data,
        };
        self.save_snapshot(&snapshot)
    }

    /// Load previously-saved window states from persistent storage.
    ///
    /// Returns an empty map if no state has been saved yet.
    pub fn load_window_states(&self) -> Result<HashMap<String, WindowState>> {
        Ok(self.load_snapshot()?.window_states)
    }

    /// Clear all persisted window state.
    pub fn clear_window_states(&self) -> Result<()> {
        remove_session_file_if_present(&self.snapshot_path())?;
        remove_session_file_if_present(&self.window_state_path())?;
        Ok(())
    }

    /// Save the entire session snapshot, including optional application data.
    pub fn save_snapshot(&self, snapshot: &SessionSnapshot) -> Result<()> {
        validate_session_snapshot(snapshot)?;
        write_json_atomically(&self.snapshot_path(), snapshot, "session snapshot")
    }

    /// Build, validate, save, and return a session snapshot.
    pub fn save_snapshot_checked(
        &self,
        snapshot: SessionSnapshotBuilder,
    ) -> Result<SessionSnapshot> {
        let snapshot = snapshot.build_checked()?;
        self.save_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Load the full session snapshot.
    ///
    /// Falls back to the legacy `window_state.json` format for compatibility.
    pub fn load_snapshot(&self) -> Result<SessionSnapshot> {
        let snapshot_path = self.snapshot_path();
        if session_path_present(&snapshot_path)? {
            let json = read_session_file(&snapshot_path, "session snapshot")?;
            let snapshot: SessionSnapshot =
                serde_json::from_str(&json).context("failed to deserialize session snapshot")?;
            validate_session_snapshot(&snapshot)?;
            return Ok(snapshot);
        }

        let legacy_path = self.window_state_path();
        if session_path_present(&legacy_path)? {
            let json = read_session_file(&legacy_path, "legacy window states")?;
            let states: HashMap<String, WindowState> = serde_json::from_str(&json)
                .context("failed to deserialize legacy window states")?;
            let snapshot = SessionSnapshot {
                window_states: states,
                app_data: None,
            };
            validate_session_snapshot(&snapshot)?;
            return Ok(snapshot);
        }

        Ok(SessionSnapshot::default())
    }

    /// Clear any persisted session snapshot and compatibility state.
    pub fn clear_snapshot(&self) -> Result<()> {
        self.clear_window_states()
    }

    /// Relocate window states whose display is no longer available to the
    /// primary display.
    ///
    /// `available_display_ids` should contain the IDs of all currently
    /// connected displays. Any window state referencing a display not in this
    /// set will have its `display_id` cleared so the application can position
    /// it on the primary display on restore.
    pub fn relocate_disconnected_displays(
        &self,
        states: &mut HashMap<String, WindowState>,
        available_display_ids: &[DisplayId],
    ) {
        self.relocate_disconnected_displays_to_primary(states, available_display_ids, None);
    }

    /// Relocate window states to the provided primary display when the
    /// previously-saved display is no longer connected.
    pub fn relocate_disconnected_displays_to_primary(
        &self,
        states: &mut HashMap<String, WindowState>,
        available_display_ids: &[DisplayId],
        primary_display_id: Option<DisplayId>,
    ) {
        for state in states.values_mut() {
            if let Some(display_id) = state.display_id {
                if !available_display_ids.contains(&display_id) {
                    state.display_id = primary_display_id;
                }
            }
        }
    }

    /// Load window states and reconcile disconnected displays before restoring.
    pub fn restore_window_states(
        &self,
        available_display_ids: &[DisplayId],
        primary_display_id: Option<DisplayId>,
    ) -> Result<HashMap<String, WindowState>> {
        Ok(self
            .restore_window_states_with_summary(available_display_ids, primary_display_id)?
            .into_window_states())
    }

    /// Load window states and return a content-safe restore summary.
    pub fn restore_window_states_with_summary(
        &self,
        available_display_ids: &[DisplayId],
        primary_display_id: Option<DisplayId>,
    ) -> Result<SessionRestoreResult> {
        let mut states = self.load_window_states()?;
        let relocated_window_count = states
            .values()
            .filter(|state| {
                state
                    .display_id
                    .is_some_and(|display_id| !available_display_ids.contains(&display_id))
            })
            .count();
        self.relocate_disconnected_displays_to_primary(
            &mut states,
            available_display_ids,
            primary_display_id,
        );
        Ok(SessionRestoreResult {
            window_states: states,
            relocated_window_count,
            available_display_count: available_display_ids.len(),
            has_primary_display: primary_display_id.is_some(),
        })
    }

    fn window_state_path(&self) -> PathBuf {
        self.storage_dir.join("window_state.json")
    }

    fn snapshot_path(&self) -> PathBuf {
        self.storage_dir.join("session_snapshot.json")
    }
}

/// Returns a platform-appropriate directory for session storage.
///
/// - macOS: `~/Library/Application Support/{app_id}/sessions`
/// - Windows: `%APPDATA%/{app_id}/sessions`
/// - Linux/FreeBSD: `$XDG_DATA_HOME/{app_id}/sessions` or `~/.local/share/{app_id}/sessions`
fn session_storage_dir(app_id: &str) -> Result<PathBuf> {
    let base = crate::util::base_data_dir()?;
    Ok(base.join(app_id).join("sessions"))
}

fn validate_session_app_id(app_id: &str) -> Result<()> {
    validate_session_identifier(app_id, "session app id")?;
    let mut components = Path::new(app_id).components();
    anyhow::ensure!(
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none(),
        "session app id must be one normal path component: {app_id:?}"
    );
    anyhow::ensure!(
        !app_id.contains(std::path::MAIN_SEPARATOR),
        "session app id cannot contain path separators: {app_id:?}"
    );
    anyhow::ensure!(
        !app_id.contains('/') && !app_id.contains('\\'),
        "session app id cannot contain path separators: {app_id:?}"
    );
    Ok(())
}

fn validate_session_window_id(id: &str) -> Result<()> {
    validate_session_identifier(id, "session window id")?;
    anyhow::ensure!(
        !id.contains('/') && !id.contains('\\'),
        "session window id cannot contain path separators: {id:?}"
    );
    Ok(())
}

fn validate_session_identifier(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value.trim() == value,
        "{label} cannot have leading or trailing whitespace: {value:?}"
    );
    anyhow::ensure!(
        value.len() <= 128,
        "{label} cannot be longer than 128 bytes: {value:?}"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters: {value:?}"
    );
    Ok(())
}

/// A persisted snapshot of the entire session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Window states keyed by application-defined identifiers.
    pub window_states: HashMap<String, WindowState>,
    /// Optional application-specific session data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_data: Option<serde_json::Value>,
}

impl SessionSnapshot {
    /// Number of persisted window states.
    pub fn window_count(&self) -> usize {
        self.window_states.len()
    }

    /// Number of persisted windows bound to a display.
    pub fn display_bound_window_count(&self) -> usize {
        self.window_states
            .values()
            .filter(|state| state.display_id.is_some())
            .count()
    }

    /// Number of persisted fullscreen windows.
    pub fn fullscreen_window_count(&self) -> usize {
        self.window_states
            .values()
            .filter(|state| state.fullscreen || matches!(state.bounds, WindowBounds::Fullscreen(_)))
            .count()
    }

    /// Whether application-specific session data is present.
    pub fn has_app_data(&self) -> bool {
        self.app_data.is_some()
    }

    /// Coarse JSON shape for application-specific session data.
    pub fn app_data_kind(&self) -> &'static str {
        session_app_data_kind(self.app_data.as_ref())
    }

    /// Content-safe summary for session restore logs and AI-agent audits.
    pub fn to_text(&self) -> String {
        format!(
            "session_snapshot windows={} display_bound={} fullscreen={} app_data={} app_data_kind={} bounds={}",
            self.window_count(),
            self.display_bound_window_count(),
            self.fullscreen_window_count(),
            self.has_app_data(),
            self.app_data_kind(),
            session_window_bounds_summary(self.window_states.values())
        )
    }
}

/// Builder for composing a persisted [`SessionSnapshot`].
#[derive(Debug, Clone, Default)]
pub struct SessionSnapshotBuilder {
    window_states: HashMap<String, WindowState>,
    app_data: Option<serde_json::Value>,
}

impl SessionSnapshotBuilder {
    /// Create an empty session snapshot builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start from an existing snapshot.
    pub fn from_snapshot(snapshot: SessionSnapshot) -> Self {
        Self {
            window_states: snapshot.window_states,
            app_data: snapshot.app_data,
        }
    }

    /// Add or replace one window state by an application-defined id.
    pub fn window_state(mut self, id: impl Into<String>, state: WindowState) -> Self {
        self.window_states.insert(id.into(), state);
        self
    }

    /// Add or replace many window states.
    pub fn window_states(
        mut self,
        states: impl IntoIterator<Item = (impl Into<String>, WindowState)>,
    ) -> Self {
        for (id, state) in states {
            self.window_states.insert(id.into(), state);
        }
        self
    }

    /// Set arbitrary JSON-serializable application session data.
    pub fn app_data<T: Serialize>(mut self, data: T) -> Result<Self> {
        self.app_data =
            Some(serde_json::to_value(data).context("failed to serialize session app data")?);
        Ok(self)
    }

    /// Set already-serialized application session data.
    pub fn app_data_value(mut self, value: serde_json::Value) -> Self {
        self.app_data = Some(value);
        self
    }

    /// Remove application-specific session data from the snapshot.
    pub fn clear_app_data(mut self) -> Self {
        self.app_data = None;
        self
    }

    /// Return the configured window states.
    pub fn configured_window_states(&self) -> &HashMap<String, WindowState> {
        &self.window_states
    }

    /// Return the configured application data, if any.
    pub fn configured_app_data(&self) -> Option<&serde_json::Value> {
        self.app_data.as_ref()
    }

    /// Number of configured window states.
    pub fn window_count(&self) -> usize {
        self.window_states.len()
    }

    /// Whether application-specific session data is configured.
    pub fn has_app_data(&self) -> bool {
        self.app_data.is_some()
    }

    /// Coarse JSON shape for configured application-specific session data.
    pub fn app_data_kind(&self) -> &'static str {
        session_app_data_kind(self.app_data.as_ref())
    }

    /// Content-safe summary for generated session snapshots.
    pub fn to_text(&self) -> String {
        format!(
            "session_snapshot_builder windows={} display_bound={} fullscreen={} app_data={} app_data_kind={} bounds={}",
            self.window_count(),
            self.window_states
                .values()
                .filter(|state| state.display_id.is_some())
                .count(),
            self.window_states
                .values()
                .filter(|state| {
                    state.fullscreen || matches!(state.bounds, WindowBounds::Fullscreen(_))
                })
                .count(),
            self.has_app_data(),
            self.app_data_kind(),
            session_window_bounds_summary(self.window_states.values())
        )
    }

    /// Validate configured window IDs and application data shape.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.window_states.len() <= MAX_SESSION_WINDOWS,
            "session cannot contain more than {MAX_SESSION_WINDOWS} windows"
        );
        for id in self.window_states.keys() {
            validate_session_window_id(id)?;
        }
        if matches!(self.app_data, Some(serde_json::Value::Null)) {
            anyhow::bail!("session app data cannot be null; call clear_app_data() instead");
        }
        if let Some(app_data) = &self.app_data {
            validate_session_json(app_data)?;
        }
        Ok(())
    }

    /// Build a validated session snapshot.
    pub fn build_checked(self) -> Result<SessionSnapshot> {
        self.validate()?;
        Ok(self.build())
    }

    /// Build the session snapshot.
    pub fn build(self) -> SessionSnapshot {
        SessionSnapshot {
            window_states: self.window_states,
            app_data: self.app_data,
        }
    }
}

impl From<SessionSnapshot> for SessionSnapshotBuilder {
    fn from(value: SessionSnapshot) -> Self {
        Self::from_snapshot(value)
    }
}

impl From<SessionSnapshotBuilder> for SessionSnapshot {
    fn from(value: SessionSnapshotBuilder) -> Self {
        value.build()
    }
}

/// Window-state restore output with a content-safe relocation summary.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRestoreResult {
    window_states: HashMap<String, WindowState>,
    relocated_window_count: usize,
    available_display_count: usize,
    has_primary_display: bool,
}

impl SessionRestoreResult {
    /// Restored window states keyed by application-defined identifiers.
    pub fn window_states(&self) -> &HashMap<String, WindowState> {
        &self.window_states
    }

    /// Consume the result and return restored window states.
    pub fn into_window_states(self) -> HashMap<String, WindowState> {
        self.window_states
    }

    /// Number of restored window states.
    pub fn window_count(&self) -> usize {
        self.window_states.len()
    }

    /// Number of windows moved away from disconnected displays.
    pub fn relocated_window_count(&self) -> usize {
        self.relocated_window_count
    }

    /// Number of connected displays considered during restore.
    pub fn available_display_count(&self) -> usize {
        self.available_display_count
    }

    /// Whether a primary display fallback was supplied.
    pub fn has_primary_display(&self) -> bool {
        self.has_primary_display
    }

    /// Whether any restored window was relocated.
    pub fn has_relocations(&self) -> bool {
        self.relocated_window_count > 0
    }

    /// Content-safe summary for restore logs and AI-agent audits.
    pub fn to_text(&self) -> String {
        format!(
            "session_restore windows={} relocated={} displays={} primary_display={} bounds={}",
            self.window_count(),
            self.relocated_window_count(),
            self.available_display_count(),
            self.has_primary_display(),
            session_window_bounds_summary(self.window_states.values())
        )
    }
}

fn session_app_data_kind(value: Option<&serde_json::Value>) -> &'static str {
    match value {
        None => "none",
        Some(serde_json::Value::Null) => "null",
        Some(serde_json::Value::Bool(_)) => "bool",
        Some(serde_json::Value::Number(_)) => "number",
        Some(serde_json::Value::String(_)) => "string",
        Some(serde_json::Value::Array(_)) => "array",
        Some(serde_json::Value::Object(_)) => "object",
    }
}

fn session_window_bounds_summary<'a>(states: impl IntoIterator<Item = &'a WindowState>) -> String {
    let mut windowed = 0;
    let mut maximized = 0;
    let mut fullscreen = 0;
    for state in states {
        match state.bounds {
            WindowBounds::Windowed(_) => windowed += 1,
            WindowBounds::Maximized(_) => maximized += 1,
            WindowBounds::Fullscreen(_) => fullscreen += 1,
        }
    }
    format!("windowed:{windowed},maximized:{maximized},fullscreen:{fullscreen}")
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let temp_path = path.with_extension(format!("json.tmp.{}", uuid::Uuid::new_v4()));
    let json = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to serialize {label}"))?;
    anyhow::ensure!(
        json.len() as u64 <= MAX_SESSION_BYTES,
        "serialized {label} exceeds {MAX_SESSION_BYTES} byte limit"
    );
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp_path)
        .with_context(|| format!("failed to create {label} at {}", temp_path.display()))?;
    let write_result = file
        .write_all(json.as_bytes())
        .with_context(|| format!("failed to write {label} to {}", temp_path.display()))
        .and_then(|()| {
            file.sync_all()
                .with_context(|| format!("failed to sync {label} at {}", temp_path.display()))
        });
    if let Err(error) = write_result {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    drop(file);
    if let Err(error) = replace_session_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to finalize {label} from {} to {}",
                temp_path.display(),
                path.display()
            )
        });
    }
    sync_session_parent(path)?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_session_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(windows)]
fn replace_session_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return std::fs::rename(temp_path, path);
    }

    let backup_path = temp_path.with_extension("replace-backup");
    std::fs::rename(path, &backup_path)?;
    if let Err(error) = std::fs::rename(temp_path, path) {
        let _ = std::fs::rename(&backup_path, path);
        return Err(error);
    }
    let _ = std::fs::remove_file(backup_path);
    Ok(())
}

#[cfg(unix)]
fn sync_session_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("failed to sync session directory {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_session_parent(_path: &Path) -> Result<()> {
    Ok(())
}

fn validate_session_file(path: &Path, label: &str) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {label} at {}", path.display()))?;
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "{label} must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_SESSION_BYTES,
        "{label} exceeds {MAX_SESSION_BYTES} byte limit"
    );
    Ok(())
}

fn read_session_file(path: &Path, label: &str) -> Result<String> {
    validate_session_file(path, label)?;

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to open {label} at {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect open {label} at {}", path.display()))?;
    anyhow::ensure!(metadata.is_file(), "{label} must be a regular file");
    anyhow::ensure!(
        metadata.len() <= MAX_SESSION_BYTES,
        "{label} exceeds {MAX_SESSION_BYTES} byte limit"
    );

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::io::Read::by_ref(&mut file)
        .take(MAX_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {label} from {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_SESSION_BYTES,
        "{label} exceeds {MAX_SESSION_BYTES} byte limit"
    );
    String::from_utf8(bytes).with_context(|| format!("{label} is not valid UTF-8"))
}

fn remove_session_file_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn session_path_present(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect session path {}", path.display()))
        }
    }
}

fn validate_session_directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect session directory: {}", path.display()))?;
    anyhow::ensure!(
        metadata.file_type().is_dir(),
        "session storage path must be a real directory: {}",
        path.display()
    );
    Ok(())
}

#[cfg(unix)]
fn restrict_session_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to secure session directory: {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_session_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn validate_session_snapshot(snapshot: &SessionSnapshot) -> Result<()> {
    anyhow::ensure!(
        snapshot.window_states.len() <= MAX_SESSION_WINDOWS,
        "session cannot contain more than {MAX_SESSION_WINDOWS} windows"
    );
    for id in snapshot.window_states.keys() {
        validate_session_window_id(id)?;
    }
    if let Some(app_data) = &snapshot.app_data {
        anyhow::ensure!(
            !app_data.is_null(),
            "session app data cannot be null; omit it instead"
        );
        validate_session_json(app_data)?;
    }
    Ok(())
}

fn validate_session_json(value: &serde_json::Value) -> Result<()> {
    const MAX_JSON_DEPTH: usize = 64;
    const MAX_JSON_NODES: usize = 100_000;
    let mut stack = vec![(value, 0usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        anyhow::ensure!(nodes <= MAX_JSON_NODES, "session app data is too complex");
        anyhow::ensure!(
            depth <= MAX_JSON_DEPTH,
            "session app data is too deeply nested"
        );
        match value {
            serde_json::Value::String(text) => anyhow::ensure!(
                text.len() as u64 <= MAX_SESSION_BYTES,
                "session app data string is too large"
            ),
            serde_json::Value::Array(values) => {
                anyhow::ensure!(
                    values.len() <= MAX_JSON_NODES.saturating_sub(nodes),
                    "session app data is too complex"
                );
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            serde_json::Value::Object(values) => {
                anyhow::ensure!(
                    values.len() <= MAX_JSON_NODES.saturating_sub(nodes),
                    "session app data is too complex"
                );
                for (key, value) in values {
                    anyhow::ensure!(
                        key.len() as u64 <= MAX_SESSION_BYTES,
                        "session app data key is too large"
                    );
                    stack.push((value, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowBounds;

    fn unique_temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kael_session_{label}_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn test_session_store_roundtrip() {
        let temp_dir =
            std::env::temp_dir().join(format!("gpui_session_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };

        let mut states = HashMap::new();
        states.insert(
            "main".to_string(),
            WindowState {
                bounds: WindowBounds::Windowed(crate::Bounds::new(
                    crate::point(crate::px(100.0), crate::px(200.0)),
                    crate::size(crate::px(800.0), crate::px(600.0)),
                )),
                display_id: Some(DisplayId(1)),
                fullscreen: false,
            },
        );

        store.save_window_states(&states).unwrap();
        let loaded = store.load_window_states().unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);
        assert_eq!(states, loaded);
    }

    #[test]
    fn session_store_rejects_path_like_app_ids() {
        assert!(SessionStore::new("../escape").is_err());
        assert!(SessionStore::new("nested/app").is_err());
        assert!(SessionStore::new(".").is_err());
        assert!(SessionStore::new("..").is_err());
    }

    #[test]
    fn corrupt_snapshot_is_not_overwritten_by_window_only_save() {
        let temp_dir = unique_temp_dir("corrupt");
        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        std::fs::write(store.snapshot_path(), b"not json").unwrap();
        assert!(store.save_window_states(&HashMap::new()).is_err());
        assert_eq!(std::fs::read(store.snapshot_path()).unwrap(), b"not json");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn oversized_snapshot_is_rejected_before_reading() {
        let temp_dir = unique_temp_dir("oversized");
        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        let file = std::fs::File::create(store.snapshot_path()).unwrap();
        file.set_len(MAX_SESSION_BYTES + 1).unwrap();
        assert!(store.load_snapshot().is_err());
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn invalid_utf8_snapshot_is_rejected() {
        let temp_dir = unique_temp_dir("invalid-utf8");
        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        std::fs::write(store.snapshot_path(), [0xff, 0xfe]).unwrap();

        let error = store.load_snapshot().unwrap_err();
        assert!(error.to_string().contains("not valid UTF-8"));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_reads_reject_symlinks_and_clear_removes_dangling_links() {
        use std::os::unix::fs::symlink;

        let temp_dir = unique_temp_dir("symlink");
        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        let target = temp_dir.join("target.json");
        std::fs::write(&target, b"{}").unwrap();
        symlink(&target, store.snapshot_path()).unwrap();
        assert!(store.load_snapshot().is_err());

        std::fs::remove_file(&target).unwrap();
        assert!(!store.snapshot_path().exists());
        assert!(store.load_snapshot().is_err());
        store.clear_snapshot().unwrap();
        assert!(std::fs::symlink_metadata(store.snapshot_path()).is_err());
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn saved_snapshots_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp_dir = unique_temp_dir("permissions");
        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        store.save_snapshot(&SessionSnapshot::default()).unwrap();

        let mode = std::fs::metadata(store.snapshot_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn test_relocate_disconnected_display() {
        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: PathBuf::from("/tmp"),
        };

        let mut states = HashMap::new();
        states.insert(
            "main".to_string(),
            WindowState {
                bounds: WindowBounds::Windowed(crate::Bounds::default()),
                display_id: Some(DisplayId(99)),
                fullscreen: false,
            },
        );

        store.relocate_disconnected_displays(&mut states, &[DisplayId(1), DisplayId(2)]);
        assert_eq!(states["main"].display_id, None);
    }

    #[test]
    fn test_session_snapshot_serialization_roundtrip() {
        let snapshot = SessionSnapshot {
            window_states: HashMap::new(),
            app_data: Some(serde_json::json!({ "theme": "dark" })),
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let deserialized: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, deserialized);
    }

    #[test]
    fn test_session_snapshot_builder_composes_window_and_app_data() {
        let bounds = WindowBounds::Windowed(crate::Bounds::new(
            crate::point(crate::px(100.0), crate::px(200.0)),
            crate::size(crate::px(800.0), crate::px(600.0)),
        ));
        let state = WindowState {
            bounds,
            display_id: Some(DisplayId(1)),
            fullscreen: false,
        };

        let snapshot = SessionSnapshotBuilder::new()
            .window_state("main", state.clone())
            .app_data(serde_json::json!({
                "workspace": "project-a",
                "sidebar": "files"
            }))
            .unwrap()
            .build();

        assert_eq!(snapshot.window_states["main"], state);
        assert_eq!(
            snapshot.app_data,
            Some(serde_json::json!({
                "workspace": "project-a",
                "sidebar": "files"
            }))
        );
    }

    #[test]
    fn test_session_snapshot_builder_checked_validation() {
        let state = WindowState {
            bounds: WindowBounds::Windowed(crate::Bounds::default()),
            display_id: Some(DisplayId(1)),
            fullscreen: false,
        };

        let checked = SessionSnapshotBuilder::new()
            .window_state("main", state.clone())
            .app_data(serde_json::json!({ "workspace": "project-a" }))
            .unwrap()
            .build_checked()
            .unwrap();
        assert_eq!(checked.window_states["main"], state);

        assert!(
            SessionSnapshotBuilder::new()
                .window_state("", state.clone())
                .build_checked()
                .is_err()
        );
        assert!(
            SessionSnapshotBuilder::new()
                .window_state(" main", state.clone())
                .build_checked()
                .is_err()
        );
        assert!(
            SessionSnapshotBuilder::new()
                .window_state("workspace/main", state.clone())
                .build_checked()
                .is_err()
        );
        assert!(
            SessionSnapshotBuilder::new()
                .window_state("main\nwindow", state.clone())
                .build_checked()
                .is_err()
        );
        assert!(
            SessionSnapshotBuilder::new()
                .window_state("main", state)
                .app_data_value(serde_json::Value::Null)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn session_snapshot_summary_is_content_safe() {
        let state = WindowState {
            bounds: WindowBounds::Maximized(crate::Bounds::new(
                crate::point(crate::px(100.0), crate::px(200.0)),
                crate::size(crate::px(800.0), crate::px(600.0)),
            )),
            display_id: Some(DisplayId(7)),
            fullscreen: false,
        };
        let builder = SessionSnapshotBuilder::new()
            .window_state("secret-main-window", state)
            .app_data(serde_json::json!({
                "workspace": "/Users/alice/secret-project",
                "token": "super-secret-token",
                "tabs": ["customer-list"]
            }))
            .unwrap();

        let builder_summary = builder.to_text();
        assert!(builder_summary.contains("windows=1"));
        assert!(builder_summary.contains("display_bound=1"));
        assert!(builder_summary.contains("app_data=true"));
        assert!(builder_summary.contains("app_data_kind=object"));
        assert!(builder_summary.contains("maximized:1"));
        assert!(!builder_summary.contains("secret-main-window"));
        assert!(!builder_summary.contains("secret-project"));
        assert!(!builder_summary.contains("super-secret-token"));
        assert!(!builder_summary.contains("customer-list"));
        assert!(!builder_summary.contains("800"));
        assert!(!builder_summary.contains("DisplayId"));

        let snapshot = builder.build_checked().unwrap();
        let snapshot_summary = snapshot.to_text();
        assert!(snapshot_summary.contains("session_snapshot"));
        assert!(snapshot_summary.contains("windows=1"));
        assert!(snapshot_summary.contains("app_data_kind=object"));
        assert!(!snapshot_summary.contains("secret-main-window"));
        assert!(!snapshot_summary.contains("secret-project"));
        assert!(!snapshot_summary.contains("super-secret-token"));
        assert!(!snapshot_summary.contains("customer-list"));
        assert!(!snapshot_summary.contains("800"));
        assert!(!snapshot_summary.contains("DisplayId"));
    }

    #[test]
    fn test_session_store_snapshot_roundtrip() {
        let temp_dir =
            std::env::temp_dir().join(format!("gpui_session_snapshot_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        let snapshot = SessionSnapshot {
            window_states: HashMap::new(),
            app_data: Some(serde_json::json!({ "theme": "dark" })),
        };

        store.save_snapshot(&snapshot).unwrap();
        let loaded = store.load_snapshot().unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(snapshot, loaded);
    }

    #[test]
    fn test_session_store_checked_snapshot_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!(
            "gpui_session_checked_snapshot_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        let snapshot = store
            .save_snapshot_checked(
                SessionSnapshotBuilder::new()
                    .window_state(
                        "main",
                        WindowState {
                            bounds: WindowBounds::Windowed(crate::Bounds::default()),
                            display_id: Some(DisplayId(2)),
                            fullscreen: false,
                        },
                    )
                    .app_data(serde_json::json!({ "openProject": "kael" }))
                    .unwrap(),
            )
            .unwrap();
        let loaded = store.load_snapshot().unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(snapshot, loaded);
        assert!(SessionStore::new_checked("").is_err());
        assert!(SessionStore::new_checked(" bad").is_err());
        assert!(SessionStore::new_checked("bad/app").is_err());
    }

    #[test]
    fn test_session_store_builder_snapshot_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!(
            "gpui_session_builder_snapshot_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };
        let snapshot = SessionSnapshotBuilder::new()
            .window_state(
                "main",
                WindowState {
                    bounds: WindowBounds::Windowed(crate::Bounds::default()),
                    display_id: Some(DisplayId(2)),
                    fullscreen: false,
                },
            )
            .app_data(serde_json::json!({ "openProject": "kael" }))
            .unwrap()
            .build();

        store.save_snapshot(&snapshot).unwrap();
        let loaded = store.load_snapshot().unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(snapshot, loaded);
    }

    #[test]
    fn test_restore_window_states_relocates_to_primary_display() {
        let temp_dir =
            std::env::temp_dir().join(format!("gpui_session_restore_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };

        let mut states = HashMap::new();
        states.insert(
            "main".to_string(),
            WindowState {
                bounds: WindowBounds::Windowed(crate::Bounds::default()),
                display_id: Some(DisplayId(99)),
                fullscreen: false,
            },
        );
        store.save_window_states(&states).unwrap();

        let restored = store
            .restore_window_states(&[DisplayId(1), DisplayId(2)], Some(DisplayId(1)))
            .unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(restored["main"].display_id, Some(DisplayId(1)));
    }

    #[test]
    fn session_restore_result_summary_is_content_safe() {
        let temp_dir = std::env::temp_dir().join(format!(
            "gpui_session_restore_summary_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "secret-app-id".to_string(),
            storage_dir: temp_dir.clone(),
        };

        let mut states = HashMap::new();
        states.insert(
            "private-workspace-window".to_string(),
            WindowState {
                bounds: WindowBounds::Fullscreen(crate::Bounds::default()),
                display_id: Some(DisplayId(99)),
                fullscreen: true,
            },
        );
        store.save_window_states(&states).unwrap();

        let restored = store
            .restore_window_states_with_summary(&[DisplayId(1), DisplayId(2)], Some(DisplayId(1)))
            .unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(restored.window_count(), 1);
        assert_eq!(restored.relocated_window_count(), 1);
        assert_eq!(restored.available_display_count(), 2);
        assert!(restored.has_primary_display());
        assert!(restored.has_relocations());
        assert_eq!(
            restored.window_states()["private-workspace-window"].display_id,
            Some(DisplayId(1))
        );

        let summary = restored.to_text();
        assert!(summary.contains("windows=1"));
        assert!(summary.contains("relocated=1"));
        assert!(summary.contains("displays=2"));
        assert!(summary.contains("primary_display=true"));
        assert!(summary.contains("fullscreen:1"));
        assert!(!summary.contains("private-workspace-window"));
        assert!(!summary.contains("secret-app-id"));
        assert!(!summary.contains("DisplayId"));
    }

    #[test]
    fn test_load_snapshot_falls_back_to_legacy_window_state_file() {
        let temp_dir =
            std::env::temp_dir().join(format!("gpui_session_legacy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let store = SessionStore {
            app_id: "test-app".to_string(),
            storage_dir: temp_dir.clone(),
        };

        let mut states = HashMap::new();
        states.insert(
            "main".to_string(),
            WindowState {
                bounds: WindowBounds::Windowed(crate::Bounds::default()),
                display_id: Some(DisplayId(7)),
                fullscreen: false,
            },
        );
        std::fs::write(
            store.window_state_path(),
            serde_json::to_string(&states).unwrap(),
        )
        .unwrap();

        let snapshot = store.load_snapshot().unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);

        assert_eq!(snapshot.window_states, states);
        assert_eq!(snapshot.app_data, None);
    }
}
