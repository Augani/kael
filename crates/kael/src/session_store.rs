use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::{DisplayId, WindowState};

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
        let storage_dir = session_storage_dir(&app_id)?;
        std::fs::create_dir_all(&storage_dir).with_context(|| {
            format!(
                "failed to create session storage directory: {}",
                storage_dir.display()
            )
        })?;

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
        let snapshot = SessionSnapshot {
            window_states: states.clone(),
            app_data: self
                .load_snapshot()
                .ok()
                .and_then(|snapshot| snapshot.app_data),
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
        let snapshot_path = self.snapshot_path();
        if snapshot_path.exists() {
            std::fs::remove_file(&snapshot_path)
                .with_context(|| format!("failed to remove {}", snapshot_path.display()))?;
        }

        let legacy_path = self.window_state_path();
        if legacy_path.exists() {
            std::fs::remove_file(&legacy_path)
                .with_context(|| format!("failed to remove {}", legacy_path.display()))?;
        }
        Ok(())
    }

    /// Save the entire session snapshot, including optional application data.
    pub fn save_snapshot(&self, snapshot: &SessionSnapshot) -> Result<()> {
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
        if snapshot_path.exists() {
            let json = std::fs::read_to_string(&snapshot_path).with_context(|| {
                format!(
                    "failed to read session snapshot from {}",
                    snapshot_path.display()
                )
            })?;
            return serde_json::from_str(&json).context("failed to deserialize session snapshot");
        }

        let legacy_path = self.window_state_path();
        if legacy_path.exists() {
            let json = std::fs::read_to_string(&legacy_path).with_context(|| {
                format!(
                    "failed to read legacy window states from {}",
                    legacy_path.display()
                )
            })?;
            let states: HashMap<String, WindowState> = serde_json::from_str(&json)
                .context("failed to deserialize legacy window states")?;
            return Ok(SessionSnapshot {
                window_states: states,
                app_data: None,
            });
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
        let mut states = self.load_window_states()?;
        self.relocate_disconnected_displays_to_primary(
            &mut states,
            available_display_ids,
            primary_display_id,
        );
        Ok(states)
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

    /// Validate configured window IDs and application data shape.
    pub fn validate(&self) -> Result<()> {
        for id in self.window_states.keys() {
            validate_session_window_id(id)?;
        }
        if matches!(self.app_data, Some(serde_json::Value::Null)) {
            anyhow::bail!("session app data cannot be null; call clear_app_data() instead");
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

fn write_json_atomically<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let temp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to serialize {label}"))?;
    std::fs::write(&temp_path, json)
        .with_context(|| format!("failed to write {label} to {}", temp_path.display()))?;
    std::fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to finalize {label} from {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowBounds;

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
