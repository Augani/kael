//! Higher-level app-runtime primitives for common desktop product patterns.
//!
//! This module provides settings storage with schema migration, a command
//! registry, and undo/redo transaction boundaries. These primitives are
//! designed to reduce boilerplate across GPUI applications.

use std::{
    any::Any,
    collections::{HashMap, VecDeque},
    fmt::Debug,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::FocusId;

// ---------------------------------------------------------------------------
// Settings Storage
// ---------------------------------------------------------------------------

/// A versioned settings store backed by a JSON file.
///
/// Settings are typed by a serializable schema. When the schema changes,
/// migrations are applied automatically based on the stored version.
#[derive(Debug, Clone)]
pub struct SettingsStore<T: Serialize + for<'de> Deserialize<'de>> {
    path: PathBuf,
    data: T,
    version: u32,
}

/// Builder for a [`SettingsStore`] with registered migrations.
pub struct SettingsStoreBuilder<T: Serialize + for<'de> Deserialize<'de> + Default> {
    path: PathBuf,
    migrations: Vec<Box<dyn SettingsMigration>>,
    _marker: std::marker::PhantomData<T>,
}

/// A migration that transforms settings from one version to the next.
pub trait SettingsMigration: Send + Sync {
    /// The version this migration targets (i.e., the version after applying).
    fn target_version(&self) -> u32;
    /// Apply the migration to a raw JSON value.
    fn migrate(&self, value: &mut serde_json::Value) -> Result<()>;
}

impl<T: Serialize + for<'de> Deserialize<'de> + Default> SettingsStore<T> {
    /// Create a builder for the given settings path.
    pub fn builder(path: impl AsRef<Path>) -> SettingsStoreBuilder<T> {
        SettingsStoreBuilder::new(path)
    }

    /// Create a new settings store at the given path with default data.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            data: T::default(),
            version: 0,
        }
    }

    /// Load settings from disk, applying migrations if needed.
    pub fn load(path: impl AsRef<Path>, migrations: &[Box<dyn SettingsMigration>]) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::new(path));
        }

        let json_str = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read settings from {}", path.display()))?;
        let mut value: serde_json::Value = serde_json::from_str(&json_str)
            .with_context(|| format!("failed to parse settings from {}", path.display()))?;

        let stored_version = value
            .get("_settings_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let mut migrations = migrations.iter().collect::<Vec<_>>();
        migrations.sort_by_key(|migration| migration.target_version());

        let mut current_version = stored_version;
        for migration in migrations {
            if current_version < migration.target_version() {
                migration.migrate(&mut value)?;
                current_version = migration.target_version();
            }
        }

        let data: T = serde_json::from_value(value.clone())
            .with_context(|| "failed to deserialize settings after migration")?;

        Ok(Self {
            path: path.to_path_buf(),
            data,
            version: current_version,
        })
    }

    /// Save the current settings to disk atomically.
    pub fn save(&self) -> Result<()> {
        let mut value = serde_json::to_value(&self.data).context("failed to serialize settings")?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "_settings_version".to_string(),
                serde_json::json!(self.version),
            );
        }
        let json = serde_json::to_string_pretty(&value).context("failed to format settings")?;
        let temp = self.path.with_extension("json.tmp");
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create settings directory {}", parent.display())
            })?;
        }
        std::fs::write(&temp, json)
            .with_context(|| format!("failed to write settings to {}", temp.display()))?;
        std::fs::rename(&temp, &self.path).with_context(|| {
            format!(
                "failed to finalize settings from {} to {}",
                temp.display(),
                self.path.display()
            )
        })?;
        Ok(())
    }

    /// Get a reference to the settings data.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get a mutable reference to the settings data.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Update settings and save atomically.
    pub fn update(&mut self, f: impl FnOnce(&mut T)) -> Result<()> {
        f(&mut self.data);
        self.save()
    }

    /// Return the path backing this store.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the schema version currently loaded in memory.
    pub fn version(&self) -> u32 {
        self.version
    }
}

impl<T: Serialize + for<'de> Deserialize<'de> + Default> SettingsStoreBuilder<T> {
    /// Create an empty settings-store builder.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            migrations: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Register a migration to be applied during load.
    pub fn migration(mut self, migration: impl SettingsMigration + 'static) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }

    /// Load the store using the configured migrations.
    pub fn load(self) -> Result<SettingsStore<T>> {
        let Self {
            path,
            migrations,
            _marker: _,
        } = self;
        SettingsStore::load(path, &migrations)
    }
}

// ---------------------------------------------------------------------------
// Command Registry
// ---------------------------------------------------------------------------

/// A globally-registered command that can be invoked by name.
///
/// Commands are used to build command palettes, menu actions, and keyboard
/// shortcuts that are decoupled from specific views.
pub trait AppCommand: Send + Sync {
    /// The unique identifier for this command.
    fn id(&self) -> &str;
    /// Human-readable display name.
    fn name(&self) -> &str;
    /// Execute the command.
    fn execute(&self);
}

/// A command backed by a closure.
pub struct ClosureCommand {
    id: String,
    name: String,
    handler: Box<dyn Fn() + Send + Sync>,
}

impl ClosureCommand {
    /// Create a closure-backed command.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        handler: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            handler: Box::new(handler),
        }
    }
}

impl AppCommand for ClosureCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn execute(&self) {
        (self.handler)();
    }
}

/// A registry of application-wide commands.
#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn AppCommand>>,
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("command_count", &self.commands.len())
            .field("command_ids", &self.commands.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CommandRegistry {
    /// Create an empty command registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a command.
    pub fn register(&mut self, command: Box<dyn AppCommand>) {
        self.commands.insert(command.id().to_string(), command);
    }

    /// Register a closure-backed command.
    pub fn register_action(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        handler: impl Fn() + Send + Sync + 'static,
    ) {
        self.register(Box::new(ClosureCommand::new(id, name, handler)));
    }

    /// Unregister a command by identifier.
    pub fn unregister(&mut self, id: &str) -> Option<Box<dyn AppCommand>> {
        self.commands.remove(id)
    }

    /// Look up a command by identifier.
    pub fn get(&self, id: &str) -> Option<&dyn AppCommand> {
        self.commands.get(id).map(|b| b.as_ref())
    }

    /// Whether a command with the given identifier is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.commands.contains_key(id)
    }

    /// Execute a command by identifier.
    pub fn execute(&self, id: &str) -> Result<()> {
        let command = self
            .commands
            .get(id)
            .ok_or_else(|| anyhow!("command not found: {}", id))?;
        command.execute();
        Ok(())
    }

    /// Return all registered commands.
    pub fn all(&self) -> Vec<&dyn AppCommand> {
        self.commands.values().map(|b| b.as_ref()).collect()
    }

    /// Return all registered command identifiers.
    pub fn command_ids(&self) -> Vec<&str> {
        let mut ids = self.commands.keys().map(String::as_str).collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// Search commands by name substring (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&dyn AppCommand> {
        let lower = query.to_lowercase();
        self.commands
            .values()
            .filter(|cmd| cmd.name().to_lowercase().contains(&lower))
            .map(|b| b.as_ref())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Undo / Redo
// ---------------------------------------------------------------------------

/// A reversible change that can be pushed onto an undo stack.
pub trait UndoableChange: Debug + 'static {
    /// Apply the change forward.
    fn apply(&mut self);
    /// Revert the change.
    fn revert(&mut self);
    /// A human-readable description for the undo stack UI.
    fn description(&self) -> &str;
    /// Returns the focus owner associated with this change when the change is routed by focus.
    fn source_id(&self) -> Option<FocusId> {
        None
    }
    /// Downcast support for history adapters layered on top of the manager.
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug)]
struct UndoTransaction {
    description: String,
    changes: Vec<Box<dyn UndoableChange>>,
}

impl UndoableChange for UndoTransaction {
    fn apply(&mut self) {
        for change in &mut self.changes {
            change.apply();
        }
    }

    fn revert(&mut self) {
        for change in self.changes.iter_mut().rev() {
            change.revert();
        }
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn source_id(&self) -> Option<FocusId> {
        let first_source = self.changes.first()?.source_id()?;
        self.changes
            .iter()
            .all(|change| change.source_id() == Some(first_source))
            .then_some(first_source)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An undo/redo manager with configurable stack depth.
#[derive(Debug)]
pub struct UndoRedoManager {
    stack: VecDeque<Box<dyn UndoableChange>>,
    index: usize,
    max_depth: usize,
    transaction: Option<UndoTransaction>,
}

impl UndoRedoManager {
    /// Create a new manager with the given maximum undo depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            stack: VecDeque::with_capacity(max_depth),
            index: 0,
            max_depth,
            transaction: None,
        }
    }

    /// Begin a grouped undo transaction with an explicit description.
    pub fn begin_transaction(&mut self, description: impl Into<String>) {
        assert!(self.transaction.is_none(), "undo transaction already open");
        self.discard_redo();
        self.transaction = Some(UndoTransaction {
            description: description.into(),
            changes: Vec::new(),
        });
    }

    /// Commit the current grouped transaction, if any changes were recorded.
    pub fn end_transaction(&mut self) -> bool {
        let Some(transaction) = self.transaction.take() else {
            return false;
        };

        if transaction.changes.is_empty() {
            return false;
        }

        self.push_applied_change(Box::new(transaction));
        true
    }

    /// Replace the most recent committed change without reapplying it.
    pub fn replace_last(&mut self, change: Box<dyn UndoableChange>) -> bool {
        if self.transaction.is_some() || self.index == 0 || self.index != self.stack.len() {
            return false;
        }

        self.stack[self.index - 1] = change;
        true
    }

    /// Push a new change onto the undo stack.
    ///
    /// Any redoable changes after the current index are discarded.
    pub fn push(&mut self, mut change: Box<dyn UndoableChange>) {
        if let Some(transaction) = self.transaction.as_mut() {
            change.apply();
            transaction.changes.push(change);
            return;
        }

        self.discard_redo();
        change.apply();
        self.push_applied_change(change);
    }

    /// Undo the most recent change, if any.
    pub fn undo(&mut self) -> Option<&dyn UndoableChange> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        self.stack[self.index].revert();
        Some(self.stack[self.index].as_ref())
    }

    /// Redo the most recently undone change, if any.
    pub fn redo(&mut self) -> Option<&dyn UndoableChange> {
        if self.index >= self.stack.len() {
            return None;
        }
        self.stack[self.index].apply();
        self.index += 1;
        Some(self.stack[self.index - 1].as_ref())
    }

    /// Undo the most recent change when it belongs to the given source.
    pub fn undo_for_source(&mut self, source_id: FocusId) -> Option<&dyn UndoableChange> {
        self.can_undo_for_source(source_id).then(|| ())?;
        self.undo()
    }

    /// Redo the next change when it belongs to the given source.
    pub fn redo_for_source(&mut self, source_id: FocusId) -> Option<&dyn UndoableChange> {
        self.can_redo_for_source(source_id).then(|| ())?;
        self.redo()
    }

    /// Whether there is a change that can be undone.
    pub fn can_undo(&self) -> bool {
        self.index > 0
    }

    /// Whether there is a change that can be redone.
    pub fn can_redo(&self) -> bool {
        self.index < self.stack.len()
    }

    /// Whether the next undoable change belongs to the given source.
    pub fn can_undo_for_source(&self, source_id: FocusId) -> bool {
        self.index
            .checked_sub(1)
            .and_then(|ix| self.stack.get(ix))
            .is_some_and(|change| change.source_id() == Some(source_id))
    }

    /// Whether any committed undoable change belongs to the given source.
    pub fn has_undo_for_source(&self, source_id: FocusId) -> bool {
        self.stack
            .iter()
            .take(self.index)
            .any(|change| change.source_id() == Some(source_id))
    }

    /// Whether the next redoable change belongs to the given source.
    pub fn can_redo_for_source(&self, source_id: FocusId) -> bool {
        self.stack
            .get(self.index)
            .is_some_and(|change| change.source_id() == Some(source_id))
    }

    /// Whether any redoable change belongs to the given source.
    pub fn has_redo_for_source(&self, source_id: FocusId) -> bool {
        self.stack
            .iter()
            .skip(self.index)
            .any(|change| change.source_id() == Some(source_id))
    }

    /// The current number of changes in the stack.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Clear all changes.
    pub fn clear(&mut self) {
        self.stack.clear();
        self.index = 0;
        self.transaction = None;
    }

    /// Remove all committed and in-flight changes that belong to the given source.
    pub fn clear_for_source(&mut self, source_id: FocusId) -> bool {
        let mut removed = false;
        let mut next_stack = VecDeque::with_capacity(self.stack.len());
        let mut next_index = 0;

        for (ix, change) in self.stack.drain(..).enumerate() {
            if change.source_id() == Some(source_id) {
                removed = true;
                continue;
            }

            if ix < self.index {
                next_index += 1;
            }
            next_stack.push_back(change);
        }

        self.stack = next_stack;
        self.index = next_index;

        if self
            .transaction
            .as_ref()
            .is_some_and(|transaction| transaction.source_id() == Some(source_id))
        {
            self.transaction = None;
            removed = true;
        }

        removed
    }

    /// Return the descriptions of undoable changes (most recent first).
    pub fn undo_descriptions(&self) -> Vec<&str> {
        self.stack
            .iter()
            .take(self.index)
            .rev()
            .map(|c| c.description())
            .collect()
    }

    /// Return the descriptions of redoable changes (next first).
    pub fn redo_descriptions(&self) -> Vec<&str> {
        self.stack
            .iter()
            .skip(self.index)
            .map(|c| c.description())
            .collect()
    }
}

impl UndoRedoManager {
    fn discard_redo(&mut self) {
        while self.stack.len() > self.index {
            self.stack.pop_back();
        }
    }

    fn push_applied_change(&mut self, change: Box<dyn UndoableChange>) {
        self.stack.push_back(change);
        self.index += 1;

        if self.stack.len() > self.max_depth {
            self.stack.pop_front();
            self.index -= 1;
        }
    }
}

impl Default for UndoRedoManager {
    fn default() -> Self {
        Self::new(100)
    }
}

// ---------------------------------------------------------------------------
// Deep Link Handling
// ---------------------------------------------------------------------------

/// Handles deep link URLs for a specific scheme.
pub trait DeepLinkHandler: Send + Sync {
    /// Returns the URL scheme this handler supports.
    fn scheme(&self) -> &str;
    /// Handles the given deep link URL.
    fn handle(&self, url: &str);
}

/// Registry for deep link handlers.
pub struct DeepLinkRegistry {
    handlers: HashMap<String, Box<dyn DeepLinkHandler>>,
}

impl DeepLinkRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Registers a deep link handler.
    pub fn register(&mut self, handler: Box<dyn DeepLinkHandler>) {
        self.handlers.insert(handler.scheme().to_string(), handler);
    }

    /// Dispatches a URL to the appropriate handler if one is registered.
    pub fn handle(&self, url: &str) -> bool {
        let scheme = url.split("://").next().unwrap_or(url);
        if let Some(handler) = self.handlers.get(scheme) {
            handler.handle(url);
            true
        } else {
            false
        }
    }
}

impl Default for DeepLinkRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Single Instance Router
// ---------------------------------------------------------------------------

/// Ensures only one instance of the application is running and routes deep links to it.
pub struct SingleInstanceRouter {
    app_id: String,
    deep_link_registry: DeepLinkRegistry,
}

impl SingleInstanceRouter {
    /// Creates a new router for the given application ID.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            deep_link_registry: DeepLinkRegistry::new(),
        }
    }

    /// Returns the application ID.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Registers a deep link handler.
    pub fn register_deep_link_handler(&mut self, handler: Box<dyn DeepLinkHandler>) {
        self.deep_link_registry.register(handler);
    }

    /// Dispatches a URL to the registered deep link handlers.
    /// Dispatches a URL to the registered handler.
    pub fn dispatch(&self, url: &str) -> bool {
        self.deep_link_registry.handle(url)
    }

    /// Attempts to acquire single-instance lock and returns a router.
    pub fn try_acquire(
        app_id: impl Into<String>,
    ) -> std::result::Result<Self, crate::platform::single_instance::AlreadyRunning> {
        let app_id = app_id.into();
        let _ = crate::platform::single_instance::SingleInstance::acquire(&app_id)?;
        Ok(Self {
            app_id,
            deep_link_registry: DeepLinkRegistry::new(),
        })
    }

    /// Sends an activate message to an existing application instance.
    pub fn send_to_existing(app_id: &str) -> Result<()> {
        crate::platform::single_instance::send_activate_to_existing(app_id)
    }
}

// ---------------------------------------------------------------------------
// Tray First Lifecycle
// ---------------------------------------------------------------------------

/// Controls whether the app window shows on launch and hides on close.
pub struct TrayFirstLifecycle {
    show_on_launch: bool,
    hide_on_close: bool,
}

impl TrayFirstLifecycle {
    /// Creates the default lifecycle configuration.
    pub fn new() -> Self {
        Self {
            show_on_launch: false,
            hide_on_close: true,
        }
    }

    /// Sets whether the window should show on launch.
    pub fn show_on_launch(mut self, show: bool) -> Self {
        self.show_on_launch = show;
        self
    }

    /// Sets whether the window should hide instead of close.
    pub fn hide_on_close(mut self, hide: bool) -> Self {
        self.hide_on_close = hide;
        self
    }

    /// Returns whether the window should show on launch.
    pub fn should_show_on_launch(&self) -> bool {
        self.show_on_launch
    }

    /// Returns whether the window should hide on close.
    pub fn should_hide_on_close(&self) -> bool {
        self.hide_on_close
    }
}

impl Default for TrayFirstLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Reopen Handler
// ---------------------------------------------------------------------------

/// Callback invoked when the application is reopened (e.g., from the dock).
pub struct ReopenHandler {
    on_reopen: Option<Box<dyn FnMut() + Send + 'static>>,
}

impl ReopenHandler {
    /// Creates a new handler with no callback.
    pub fn new() -> Self {
        Self { on_reopen: None }
    }

    /// Sets the callback to invoke on reopen.
    pub fn on_reopen(mut self, callback: impl FnMut() + Send + 'static) -> Self {
        self.on_reopen = Some(Box::new(callback));
        self
    }

    /// Triggers the reopen callback if one is set.
    pub fn trigger(&mut self) {
        if let Some(ref mut callback) = self.on_reopen {
            callback();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_settings_store_roundtrip() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct MySettings {
            theme: String,
            font_size: u32,
        }

        let temp = std::env::temp_dir().join(format!("kael_settings_test_{}", std::process::id()));
        let mut store: SettingsStore<MySettings> = SettingsStore::new(&temp);
        store.data_mut().theme = "dark".to_string();
        store.data_mut().font_size = 14;
        store.save().unwrap();

        let loaded: SettingsStore<MySettings> = SettingsStore::load(&temp, &[]).unwrap();
        assert_eq!(loaded.data().theme, "dark");
        assert_eq!(loaded.data().font_size, 14);

        let _ = std::fs::remove_file(&temp);
    }

    #[test]
    fn test_settings_migration() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct V2Settings {
            appearance: String,
            font_size: u32,
        }

        struct V1ToV2Migration;
        impl SettingsMigration for V1ToV2Migration {
            fn target_version(&self) -> u32 {
                2
            }
            fn migrate(&self, value: &mut serde_json::Value) -> Result<()> {
                if let Some(theme) = value.get("theme").cloned() {
                    value
                        .as_object_mut()
                        .unwrap()
                        .insert("appearance".to_string(), theme);
                }
                Ok(())
            }
        }

        let temp = std::env::temp_dir().join(format!("kael_migrate_test_{}", std::process::id()));
        let json = r#"{"theme":"light","font_size":12}"#;
        std::fs::write(&temp, json).unwrap();

        let loaded: SettingsStore<V2Settings> =
            SettingsStore::load(&temp, &[Box::new(V1ToV2Migration)]).unwrap();
        assert_eq!(loaded.data().appearance, "light");
        assert_eq!(loaded.data().font_size, 12);

        let _ = std::fs::remove_file(&temp);
    }

    #[test]
    fn test_settings_store_builder() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct BuiltSettings {
            enabled: bool,
        }

        let temp = std::env::temp_dir().join(format!("gpui_builder_test_{}", std::process::id()));
        let store = SettingsStore::<BuiltSettings>::builder(&temp)
            .load()
            .unwrap();

        assert_eq!(store.path(), temp.as_path());
        assert_eq!(store.version(), 0);

        let _ = std::fs::remove_file(&temp);
    }

    #[test]
    fn test_command_registry() {
        struct SayHello;
        impl AppCommand for SayHello {
            fn id(&self) -> &str {
                "hello"
            }
            fn name(&self) -> &str {
                "Say Hello"
            }
            fn execute(&self) {}
        }

        let mut registry = CommandRegistry::new();
        registry.register(Box::new(SayHello));

        assert!(registry.get("hello").is_some());
        assert!(registry.get("missing").is_none());
        assert_eq!(registry.search("hello").len(), 1);
        assert_eq!(registry.search("world").len(), 0);
    }

    #[test]
    fn test_command_registry_register_action() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let triggered = Arc::new(AtomicBool::new(false));
        let mut registry = CommandRegistry::new();
        registry.register_action("save", "Save", {
            let triggered = Arc::clone(&triggered);
            move || {
                triggered.store(true, Ordering::Relaxed);
            }
        });

        assert!(registry.contains("save"));
        assert_eq!(registry.command_ids(), vec!["save"]);
        registry.execute("save").unwrap();
        assert!(triggered.load(Ordering::Relaxed));
    }

    #[test]
    fn test_undo_redo_basic() {
        let value = Arc::new(Mutex::new(0));

        struct AddChange {
            value: Arc<Mutex<i32>>,
            delta: i32,
            desc: String,
        }

        impl Debug for AddChange {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("AddChange")
                    .field("delta", &self.delta)
                    .finish()
            }
        }

        impl UndoableChange for AddChange {
            fn apply(&mut self) {
                *self.value.lock().unwrap() += self.delta;
            }
            fn revert(&mut self) {
                *self.value.lock().unwrap() -= self.delta;
            }
            fn description(&self) -> &str {
                &self.desc
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut undo = UndoRedoManager::new(10);
        undo.push(Box::new(AddChange {
            value: value.clone(),
            delta: 5,
            desc: "add 5".to_string(),
        }));
        assert_eq!(*value.lock().unwrap(), 5);

        undo.push(Box::new(AddChange {
            value: value.clone(),
            delta: 3,
            desc: "add 3".to_string(),
        }));
        assert_eq!(*value.lock().unwrap(), 8);

        undo.undo();
        assert_eq!(*value.lock().unwrap(), 5);

        undo.redo();
        assert_eq!(*value.lock().unwrap(), 8);

        undo.undo();
        undo.undo();
        assert_eq!(*value.lock().unwrap(), 0);
        assert!(!undo.can_undo());
    }

    #[test]
    fn test_undo_redo_max_depth() {
        let value = Arc::new(Mutex::new(0));

        struct IncChange {
            value: Arc<Mutex<i32>>,
        }
        impl Debug for IncChange {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("IncChange").finish()
            }
        }
        impl UndoableChange for IncChange {
            fn apply(&mut self) {
                *self.value.lock().unwrap() += 1;
            }
            fn revert(&mut self) {
                *self.value.lock().unwrap() -= 1;
            }
            fn description(&self) -> &str {
                "inc"
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut undo = UndoRedoManager::new(2);
        undo.push(Box::new(IncChange {
            value: value.clone(),
        }));
        undo.push(Box::new(IncChange {
            value: value.clone(),
        }));
        undo.push(Box::new(IncChange {
            value: value.clone(),
        }));

        assert_eq!(undo.len(), 2);
        assert_eq!(*value.lock().unwrap(), 3);
    }

    #[test]
    fn test_undo_redo_discards_redo_on_push() {
        let value = Arc::new(Mutex::new(0));

        struct SetChange {
            value: Arc<Mutex<i32>>,
            target: i32,
            prev: i32,
        }
        impl Debug for SetChange {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("SetChange")
                    .field("target", &self.target)
                    .finish()
            }
        }
        impl UndoableChange for SetChange {
            fn apply(&mut self) {
                self.prev = *self.value.lock().unwrap();
                *self.value.lock().unwrap() = self.target;
            }
            fn revert(&mut self) {
                *self.value.lock().unwrap() = self.prev;
            }
            fn description(&self) -> &str {
                "set"
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut undo = UndoRedoManager::new(10);
        undo.push(Box::new(SetChange {
            value: value.clone(),
            target: 10,
            prev: 0,
        }));
        undo.push(Box::new(SetChange {
            value: value.clone(),
            target: 20,
            prev: 10,
        }));

        undo.undo();
        assert_eq!(*value.lock().unwrap(), 10);

        undo.push(Box::new(SetChange {
            value: value.clone(),
            target: 30,
            prev: 10,
        }));
        assert!(!undo.can_redo());
        assert_eq!(*value.lock().unwrap(), 30);
    }

    #[test]
    fn test_undo_redo_transactions_group_multiple_changes() {
        let value = Arc::new(Mutex::new(0));

        struct AddChange {
            value: Arc<Mutex<i32>>,
            delta: i32,
        }

        impl Debug for AddChange {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("AddChange")
                    .field("delta", &self.delta)
                    .finish()
            }
        }

        impl UndoableChange for AddChange {
            fn apply(&mut self) {
                *self.value.lock().unwrap() += self.delta;
            }

            fn revert(&mut self) {
                *self.value.lock().unwrap() -= self.delta;
            }

            fn description(&self) -> &str {
                "add"
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut undo = UndoRedoManager::new(10);
        undo.begin_transaction("compound add");
        undo.push(Box::new(AddChange {
            value: value.clone(),
            delta: 2,
        }));
        undo.push(Box::new(AddChange {
            value: value.clone(),
            delta: 3,
        }));

        assert_eq!(*value.lock().unwrap(), 5);
        assert_eq!(undo.len(), 0);
        assert!(undo.end_transaction());
        assert_eq!(undo.len(), 1);
        assert_eq!(undo.undo_descriptions(), vec!["compound add"]);

        undo.undo();
        assert_eq!(*value.lock().unwrap(), 0);

        undo.redo();
        assert_eq!(*value.lock().unwrap(), 5);
    }

    #[test]
    fn test_undo_redo_replace_last_change() {
        let value = Arc::new(Mutex::new(0));

        struct SetChange {
            value: Arc<Mutex<i32>>,
            previous: i32,
            next: i32,
        }

        impl Debug for SetChange {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("SetChange")
                    .field("previous", &self.previous)
                    .field("next", &self.next)
                    .finish()
            }
        }

        impl UndoableChange for SetChange {
            fn apply(&mut self) {
                *self.value.lock().unwrap() = self.next;
            }

            fn revert(&mut self) {
                *self.value.lock().unwrap() = self.previous;
            }

            fn description(&self) -> &str {
                "set"
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut undo = UndoRedoManager::new(10);
        undo.push(Box::new(SetChange {
            value: value.clone(),
            previous: 0,
            next: 1,
        }));
        assert_eq!(*value.lock().unwrap(), 1);

        *value.lock().unwrap() = 3;
        assert!(undo.replace_last(Box::new(SetChange {
            value: value.clone(),
            previous: 0,
            next: 3,
        })));

        undo.undo();
        assert_eq!(*value.lock().unwrap(), 0);

        undo.redo();
        assert_eq!(*value.lock().unwrap(), 3);
    }

    #[test]
    fn test_undo_redo_source_targeting_requires_top_entry_ownership() {
        let value = Arc::new(Mutex::new(0));
        let mut focus_ids = SlotMap::<FocusId, ()>::with_key();
        let first_source = focus_ids.insert(());
        let second_source = focus_ids.insert(());

        struct TaggedChange {
            value: Arc<Mutex<i32>>,
            delta: i32,
            source_id: FocusId,
        }

        impl Debug for TaggedChange {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("TaggedChange")
                    .field("delta", &self.delta)
                    .finish()
            }
        }

        impl UndoableChange for TaggedChange {
            fn apply(&mut self) {
                *self.value.lock().unwrap() += self.delta;
            }

            fn revert(&mut self) {
                *self.value.lock().unwrap() -= self.delta;
            }

            fn description(&self) -> &str {
                "tagged"
            }

            fn source_id(&self) -> Option<FocusId> {
                Some(self.source_id)
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut undo = UndoRedoManager::new(10);
        undo.push(Box::new(TaggedChange {
            value: value.clone(),
            delta: 2,
            source_id: first_source,
        }));
        undo.push(Box::new(TaggedChange {
            value: value.clone(),
            delta: 7,
            source_id: second_source,
        }));

        assert_eq!(*value.lock().unwrap(), 9);
        assert!(!undo.can_undo_for_source(first_source));
        assert!(undo.can_undo_for_source(second_source));
        assert!(undo.undo_for_source(first_source).is_none());
        assert_eq!(*value.lock().unwrap(), 9);

        assert!(undo.undo_for_source(second_source).is_some());
        assert_eq!(*value.lock().unwrap(), 2);
        assert!(undo.can_redo_for_source(second_source));
        assert!(!undo.can_redo_for_source(first_source));
    }

    #[test]
    fn test_deep_link_registry() {
        struct MyHandler {
            scheme: String,
            triggered: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        impl DeepLinkHandler for MyHandler {
            fn scheme(&self) -> &str {
                &self.scheme
            }
            fn handle(&self, _url: &str) {
                self.triggered
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }

        let triggered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut registry = DeepLinkRegistry::new();
        registry.register(Box::new(MyHandler {
            scheme: "myapp".to_string(),
            triggered: triggered.clone(),
        }));

        assert!(registry.handle("myapp://open/file"));
        assert!(triggered.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!registry.handle("unknown://test"));
    }

    #[test]
    fn test_single_instance_router_dispatch() {
        let router = SingleInstanceRouter::new("test-app");
        assert_eq!(router.app_id(), "test-app");
        assert!(!router.dispatch("myapp://test"));
    }

    #[test]
    fn test_reopen_handler() {
        let triggered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut handler = ReopenHandler::new().on_reopen({
            let triggered = triggered.clone();
            move || {
                triggered.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        handler.trigger();
        assert!(triggered.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_tray_first_lifecycle() {
        let lifecycle = TrayFirstLifecycle::new()
            .show_on_launch(true)
            .hide_on_close(false);
        assert!(lifecycle.should_show_on_launch());
        assert!(!lifecycle.should_hide_on_close());
    }
}
