//! Higher-level app-runtime primitives for common desktop product patterns.
//!
//! This module provides settings storage with schema migration, a command
//! registry, and undo/redo transaction boundaries. These primitives are
//! designed to reduce boilerplate across GPUI applications.

use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
    fmt::Debug,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::FocusId;

const MAX_APP_COMMANDS: usize = 4_096;
const MAX_COMMAND_SEARCH_BYTES: usize = 512;
const MAX_SETTINGS_BYTES: usize = 8 * 1024 * 1024;
const MAX_SETTINGS_MIGRATIONS: usize = 256;
const MAX_SETTINGS_PATH_BYTES: usize = 4_096;
const MAX_UNDO_DEPTH: usize = 10_000;
const MAX_TRANSACTION_CHANGES: usize = 10_000;
const MAX_DEEP_LINK_HANDLERS: usize = 256;
const MAX_DEEP_LINK_SCHEME_BYTES: usize = 64;
const MAX_DEEP_LINK_URL_BYTES: usize = 16 * 1024;

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

    /// Create a new settings store after validating the backing path.
    pub fn new_checked(path: impl AsRef<Path>) -> Result<Self> {
        validate_settings_path(path.as_ref())?;
        Ok(Self::new(path))
    }

    /// Load settings from disk, applying migrations if needed.
    pub fn load(path: impl AsRef<Path>, migrations: &[Box<dyn SettingsMigration>]) -> Result<Self> {
        let path = path.as_ref();
        validate_settings_path(path)?;
        let mut migrations = validated_settings_migrations(migrations)?;
        if !path.exists() {
            return Ok(Self::new(path));
        }

        let json_bytes = read_settings_file(path)?;
        let mut value: serde_json::Value =
            serde_json::from_slice(&json_bytes).context("failed to parse settings JSON")?;

        let stored_version = value
            .get("_settings_version")
            .and_then(|v| v.as_u64())
            .map(u32::try_from)
            .transpose()
            .context("settings version exceeds u32")?
            .unwrap_or(0);

        migrations.sort_by_key(|(target_version, _)| *target_version);

        let mut current_version = stored_version;
        for (target_version, migration) in migrations {
            if current_version < target_version {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    migration.migrate(&mut value)
                }))
                .map_err(|_| anyhow!("settings migration panicked"))??;
                ensure_settings_value_size(&value)?;
                current_version = target_version;
            }
        }

        let data: T = serde_json::from_value(value)
            .with_context(|| "failed to deserialize settings after migration")?;

        Ok(Self {
            path: path.to_path_buf(),
            data,
            version: current_version,
        })
    }

    /// Save the current settings to disk atomically.
    pub fn save(&self) -> Result<()> {
        validate_settings_path(&self.path)?;
        let mut value = serde_json::to_value(&self.data).context("failed to serialize settings")?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "_settings_version".to_string(),
                serde_json::json!(self.version),
            );
        }
        let json = serde_json::to_vec_pretty(&value).context("failed to format settings")?;
        anyhow::ensure!(
            json.len() <= MAX_SETTINGS_BYTES,
            "settings payload cannot exceed {MAX_SETTINGS_BYTES} bytes"
        );
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).context("failed to create settings directory")?;
        }
        write_settings_atomically(&self.path, &json)
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
        let previous = serde_json::to_value(&self.data)
            .context("failed to snapshot settings before update")?;
        let update = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut self.data)));
        if update.is_err() {
            self.data = serde_json::from_value(previous)
                .context("failed to restore settings after update panic")?;
            return Err(anyhow!("settings update callback panicked"));
        }
        if let Err(error) = self.save() {
            self.data = serde_json::from_value(previous)
                .context("failed to restore settings after save failure")?;
            return Err(error);
        }
        Ok(())
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
        if self.migrations.len() <= MAX_SETTINGS_MIGRATIONS {
            self.migrations.push(Box::new(migration));
        }
        self
    }

    /// Validate the settings path and migration targets.
    pub fn validate(&self) -> Result<()> {
        validate_settings_path(&self.path)?;
        validate_settings_migrations(&self.migrations)
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

    /// Validate and load the store using the configured migrations.
    pub fn load_checked(self) -> Result<SettingsStore<T>> {
        self.validate()?;
        self.load()
    }
}

fn read_settings_file(path: &Path) -> Result<Vec<u8>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).context("failed to open settings file")?;
    let metadata = file.metadata().context("failed to inspect settings file")?;
    anyhow::ensure!(metadata.is_file(), "settings path must be a regular file");
    anyhow::ensure!(
        metadata.len() <= MAX_SETTINGS_BYTES as u64,
        "settings file cannot exceed {MAX_SETTINGS_BYTES} bytes"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_SETTINGS_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("failed to read settings file")?;
    anyhow::ensure!(
        bytes.len() <= MAX_SETTINGS_BYTES,
        "settings file cannot exceed {MAX_SETTINGS_BYTES} bytes"
    );
    Ok(bytes)
}

fn ensure_settings_value_size(value: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec(value).context("failed to size migrated settings")?;
    anyhow::ensure!(
        bytes.len() <= MAX_SETTINGS_BYTES,
        "migrated settings cannot exceed {MAX_SETTINGS_BYTES} bytes"
    );
    Ok(())
}

fn write_settings_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings");
    let temp_name = format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4().simple());
    let temp = parent.unwrap_or_else(|| Path::new(".")).join(temp_name);

    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&temp)
            .context("failed to create settings temp file")?;
        file.write_all(bytes)
            .context("failed to write settings temp file")?;
        file.sync_all()
            .context("failed to sync settings temp file")?;
        drop(file);
        std::fs::rename(&temp, path).context("failed to finalize settings file")?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn validate_settings_path(path: &Path) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "settings path cannot be empty"
    );
    let path_text = path.to_string_lossy();
    anyhow::ensure!(
        path_text.len() <= MAX_SETTINGS_PATH_BYTES,
        "settings path cannot exceed {MAX_SETTINGS_PATH_BYTES} bytes"
    );
    anyhow::ensure!(
        !path_text.chars().any(char::is_control),
        "settings path cannot contain control characters"
    );
    anyhow::ensure!(
        path.file_name()
            .is_some_and(|file_name| !file_name.is_empty()),
        "settings path must include a file name"
    );
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "settings path cannot be a symbolic link"
        );
        anyhow::ensure!(
            metadata.is_file(),
            "settings path must point to a regular file"
        );
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && parent.exists()
    {
        anyhow::ensure!(
            parent.is_dir(),
            "settings parent path must be a directory: {}",
            parent.display()
        );
    }
    Ok(())
}

fn validate_settings_migrations(migrations: &[Box<dyn SettingsMigration>]) -> Result<()> {
    validated_settings_migrations(migrations).map(|_| ())
}

fn validated_settings_migrations(
    migrations: &[Box<dyn SettingsMigration>],
) -> Result<Vec<(u32, &dyn SettingsMigration)>> {
    anyhow::ensure!(
        migrations.len() <= MAX_SETTINGS_MIGRATIONS,
        "settings migrations cannot exceed {MAX_SETTINGS_MIGRATIONS}"
    );
    let mut versions = HashSet::new();
    let mut validated = Vec::with_capacity(migrations.len());
    for migration in migrations {
        let target_version = migration_target_version(migration.as_ref())?;
        anyhow::ensure!(
            target_version > 0,
            "settings migration target version must be greater than zero"
        );
        anyhow::ensure!(
            versions.insert(target_version),
            "settings migration target version {target_version} is registered more than once"
        );
        validated.push((target_version, migration.as_ref()));
    }
    Ok(validated)
}

fn migration_target_version(migration: &dyn SettingsMigration) -> Result<u32> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| migration.target_version()))
        .map_err(|_| anyhow!("settings migration target callback panicked"))
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

    /// Validate this command before registering it in generated app chrome.
    pub fn validate(&self) -> Result<()> {
        validate_command_id(&self.id)?;
        validate_command_name(&self.name)?;
        Ok(())
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
        let _ = self.register_checked(command);
    }

    /// Register a command after validating its id/name and checking duplicates.
    pub fn register_checked(&mut self, command: Box<dyn AppCommand>) -> Result<()> {
        let (id, name) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (command.id().to_string(), command.name().to_string())
        }))
        .map_err(|_| anyhow!("command metadata callback panicked"))?;
        validate_command_id(&id)?;
        validate_command_name(&name)?;
        anyhow::ensure!(
            !self.commands.contains_key(&id),
            "command id is already registered"
        );
        anyhow::ensure!(
            self.commands.len() < MAX_APP_COMMANDS,
            "command registry cannot contain more than {MAX_APP_COMMANDS} commands"
        );
        self.commands.insert(id, command);
        Ok(())
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

    /// Register a closure-backed command after validating id/name and duplicates.
    pub fn register_action_checked(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        handler: impl Fn() + Send + Sync + 'static,
    ) -> Result<()> {
        self.register_checked(Box::new(ClosureCommand::new(id, name, handler)))
    }

    /// Unregister a command by identifier.
    pub fn unregister(&mut self, id: &str) -> Option<Box<dyn AppCommand>> {
        if validate_command_id(id).is_err() {
            return None;
        }
        self.commands.remove(id)
    }

    /// Look up a command by identifier.
    pub fn get(&self, id: &str) -> Option<&dyn AppCommand> {
        if validate_command_id(id).is_err() {
            return None;
        }
        self.commands.get(id).map(|b| b.as_ref())
    }

    /// Whether a command with the given identifier is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Execute a command by identifier.
    pub fn execute(&self, id: &str) -> Result<()> {
        validate_command_id(id)?;
        let command = self
            .commands
            .get(id)
            .ok_or_else(|| anyhow!("command not found"))?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| command.execute()))
            .map_err(|_| anyhow!("command handler panicked"))?;
        Ok(())
    }

    /// Return all registered commands.
    pub fn all(&self) -> Vec<&dyn AppCommand> {
        let mut commands = self
            .commands
            .iter()
            .map(|(id, command)| (id.as_str(), command.as_ref()))
            .collect::<Vec<_>>();
        commands.sort_unstable_by(|left, right| left.0.cmp(right.0));
        commands.into_iter().map(|(_, command)| command).collect()
    }

    /// Return all registered command identifiers.
    pub fn command_ids(&self) -> Vec<&str> {
        let mut ids = self.commands.keys().map(String::as_str).collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// Search commands by name substring (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&dyn AppCommand> {
        if query.len() > MAX_COMMAND_SEARCH_BYTES || query.chars().any(char::is_control) {
            return Vec::new();
        }
        let lower = query.to_lowercase();
        let mut commands = self
            .commands
            .iter()
            .filter(|(id, command)| {
                id.to_lowercase().contains(&lower)
                    || std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        command.name().to_lowercase().contains(&lower)
                    }))
                    .unwrap_or(false)
            })
            .map(|(id, command)| (id.as_str(), command.as_ref()))
            .collect::<Vec<_>>();
        commands.sort_unstable_by(|left, right| left.0.cmp(right.0));
        commands.into_iter().map(|(_, command)| command).collect()
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the command registry is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

fn validate_command_id(id: &str) -> Result<()> {
    anyhow::ensure!(!id.trim().is_empty(), "command id cannot be empty");
    anyhow::ensure!(
        id == id.trim(),
        "command id cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        id.len() <= 128,
        "command id cannot be longer than 128 bytes"
    );
    anyhow::ensure!(
        id.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '-' | '_' | '/')),
        "command id must contain only ASCII letters, numbers, '.', ':', '-', '_' or '/'"
    );
    Ok(())
}

fn validate_command_name(name: &str) -> Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "command name cannot be empty");
    anyhow::ensure!(
        name == name.trim(),
        "command name cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        name.chars().count() <= 128,
        "command name cannot be longer than 128 characters"
    );
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "command name cannot contain control characters"
    );
    Ok(())
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
        let max_depth = max_depth.min(MAX_UNDO_DEPTH);
        Self {
            stack: VecDeque::with_capacity(max_depth),
            index: 0,
            max_depth,
            transaction: None,
        }
    }

    /// Create a manager after validating its maximum retained depth.
    pub fn new_checked(max_depth: usize) -> Result<Self> {
        anyhow::ensure!(
            max_depth <= MAX_UNDO_DEPTH,
            "undo depth cannot exceed {MAX_UNDO_DEPTH}"
        );
        Ok(Self::new(max_depth))
    }

    /// Begin a grouped undo transaction with an explicit description.
    pub fn begin_transaction(&mut self, description: impl Into<String>) {
        let _ = self.begin_transaction_checked(description);
    }

    /// Begin a grouped undo transaction after validating generated input.
    pub fn begin_transaction_checked(&mut self, description: impl Into<String>) -> Result<()> {
        anyhow::ensure!(self.transaction.is_none(), "undo transaction already open");
        let description = description.into();
        validate_undo_description(&description)?;
        self.transaction = Some(UndoTransaction {
            description,
            changes: Vec::new(),
        });
        Ok(())
    }

    /// Commit the current grouped transaction, if any changes were recorded.
    pub fn end_transaction(&mut self) -> bool {
        let Some(transaction) = self.transaction.take() else {
            return false;
        };

        if transaction.changes.is_empty() {
            return false;
        }

        self.discard_redo();
        self.push_applied_change(Box::new(transaction));
        true
    }

    /// Commit the current grouped transaction, returning an error if none is open.
    pub fn end_transaction_checked(&mut self) -> Result<bool> {
        anyhow::ensure!(self.transaction.is_some(), "no undo transaction is open");
        Ok(self.end_transaction())
    }

    /// Replace the most recent committed change without reapplying it.
    pub fn replace_last(&mut self, change: Box<dyn UndoableChange>) -> bool {
        self.replace_last_checked(change).unwrap_or(false)
    }

    /// Replace the most recent change after validating its description.
    pub fn replace_last_checked(&mut self, change: Box<dyn UndoableChange>) -> Result<bool> {
        validate_change_description(change.as_ref())?;
        if self.transaction.is_some() || self.index == 0 || self.index != self.stack.len() {
            return Ok(false);
        }

        self.stack[self.index - 1] = change;
        Ok(true)
    }

    /// Push a new change onto the undo stack.
    ///
    /// Any redoable changes after the current index are discarded.
    pub fn push(&mut self, change: Box<dyn UndoableChange>) {
        let _ = self.push_checked(change);
    }

    /// Apply and retain a change while preserving history invariants on panic.
    pub fn push_checked(&mut self, mut change: Box<dyn UndoableChange>) -> Result<()> {
        validate_change_description(change.as_ref())?;
        if let Some(transaction) = self.transaction.as_mut() {
            anyhow::ensure!(
                transaction.changes.len() < MAX_TRANSACTION_CHANGES,
                "undo transaction cannot contain more than {MAX_TRANSACTION_CHANGES} changes"
            );
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| change.apply()))
                .map_err(|_| anyhow!("undo change apply panicked"))?;
            transaction.changes.push(change);
            return Ok(());
        }

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| change.apply()))
            .map_err(|_| anyhow!("undo change apply panicked"))?;
        self.discard_redo();
        self.push_applied_change(change);
        Ok(())
    }

    /// Undo the most recent change, if any.
    pub fn undo(&mut self) -> Option<&dyn UndoableChange> {
        self.undo_checked().ok().flatten()
    }

    /// Undo one change and surface a panicking revert without moving the history index.
    pub fn undo_checked(&mut self) -> Result<Option<&dyn UndoableChange>> {
        if self.index == 0 {
            return Ok(None);
        }
        let target = self.index - 1;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.stack[target].revert()))
            .map_err(|_| anyhow!("undo change revert panicked"))?;
        self.index = target;
        Ok(Some(self.stack[target].as_ref()))
    }

    /// Redo the most recently undone change, if any.
    pub fn redo(&mut self) -> Option<&dyn UndoableChange> {
        self.redo_checked().ok().flatten()
    }

    /// Redo one change and surface a panicking apply without moving the history index.
    pub fn redo_checked(&mut self) -> Result<Option<&dyn UndoableChange>> {
        if self.index >= self.stack.len() {
            return Ok(None);
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.stack[self.index].apply()
        }))
        .map_err(|_| anyhow!("redo change apply panicked"))?;
        self.index += 1;
        Ok(Some(self.stack[self.index - 1].as_ref()))
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
            .is_some_and(|change| safe_change_source_id(change.as_ref()) == Some(source_id))
    }

    /// Whether any committed undoable change belongs to the given source.
    pub fn has_undo_for_source(&self, source_id: FocusId) -> bool {
        self.stack
            .iter()
            .take(self.index)
            .any(|change| safe_change_source_id(change.as_ref()) == Some(source_id))
    }

    /// Whether the next redoable change belongs to the given source.
    pub fn can_redo_for_source(&self, source_id: FocusId) -> bool {
        self.stack
            .get(self.index)
            .is_some_and(|change| safe_change_source_id(change.as_ref()) == Some(source_id))
    }

    /// Whether any redoable change belongs to the given source.
    pub fn has_redo_for_source(&self, source_id: FocusId) -> bool {
        self.stack
            .iter()
            .skip(self.index)
            .any(|change| safe_change_source_id(change.as_ref()) == Some(source_id))
    }

    /// The current number of changes in the stack.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Maximum number of committed changes retained by the manager.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Number of committed changes that can currently be undone.
    pub fn undo_count(&self) -> usize {
        self.index
    }

    /// Number of committed changes that can currently be redone.
    pub fn redo_count(&self) -> usize {
        self.stack.len().saturating_sub(self.index)
    }

    /// Whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Whether a grouped transaction is currently open.
    pub fn has_open_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// Number of changes currently recorded in the open transaction.
    pub fn transaction_change_count(&self) -> usize {
        self.transaction
            .as_ref()
            .map(|transaction| transaction.changes.len())
            .unwrap_or(0)
    }

    /// Whether the committed stack has reached the configured maximum depth.
    pub fn is_at_max_depth(&self) -> bool {
        self.max_depth > 0 && self.stack.len() >= self.max_depth
    }

    /// Content-safe summary that avoids logging undo/redo descriptions.
    pub fn to_text(&self) -> String {
        format!(
            "undo redo: undo {}, redo {}, total {}, max-depth {}, at-max {}, transaction {}, transaction-changes {}",
            self.undo_count(),
            self.redo_count(),
            self.len(),
            self.max_depth(),
            self.is_at_max_depth(),
            self.has_open_transaction(),
            self.transaction_change_count()
        )
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

        let old_index = self.index;
        let mut ix = 0;
        while let Some(change) = self.stack.pop_front() {
            if safe_change_source_id(change.as_ref()) == Some(source_id) {
                removed = true;
                ix += 1;
                continue;
            }

            if ix < old_index {
                next_index += 1;
            }
            next_stack.push_back(change);
            ix += 1;
        }

        self.stack = next_stack;
        self.index = next_index;

        if self
            .transaction
            .as_ref()
            .is_some_and(|transaction| safe_change_source_id(transaction) == Some(source_id))
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
            .filter_map(|change| safe_change_description(change.as_ref()))
            .collect()
    }

    /// Return the descriptions of redoable changes (next first).
    pub fn redo_descriptions(&self) -> Vec<&str> {
        self.stack
            .iter()
            .skip(self.index)
            .filter_map(|change| safe_change_description(change.as_ref()))
            .collect()
    }
}

fn validate_undo_description(description: &str) -> Result<()> {
    anyhow::ensure!(
        !description.trim().is_empty(),
        "undo transaction description cannot be empty"
    );
    anyhow::ensure!(
        description == description.trim(),
        "undo transaction description cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        description.chars().count() <= 128,
        "undo transaction description cannot be longer than 128 characters"
    );
    anyhow::ensure!(
        !description.chars().any(char::is_control),
        "undo transaction description cannot contain control characters"
    );
    Ok(())
}

fn validate_change_description(change: &dyn UndoableChange) -> Result<()> {
    let description = safe_change_description(change)
        .ok_or_else(|| anyhow!("undo change description callback panicked"))?;
    validate_undo_description(description)
}

fn safe_change_description(change: &dyn UndoableChange) -> Option<&str> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| change.description())).ok()
}

fn safe_change_source_id(change: &dyn UndoableChange) -> Option<FocusId> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| change.source_id()))
        .ok()
        .flatten()
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
        let _ = self.register_checked(handler);
    }

    /// Registers a handler after validating its callback and URL scheme.
    pub fn register_checked(&mut self, handler: Box<dyn DeepLinkHandler>) -> Result<()> {
        let scheme =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.scheme().to_owned()))
                .map_err(|_| anyhow!("deep-link scheme callback panicked"))?;
        validate_deep_link_scheme(&scheme)?;
        let scheme = scheme.to_ascii_lowercase();
        anyhow::ensure!(
            !self.handlers.contains_key(&scheme),
            "deep-link scheme is already registered"
        );
        anyhow::ensure!(
            self.handlers.len() < MAX_DEEP_LINK_HANDLERS,
            "deep-link registry cannot exceed {MAX_DEEP_LINK_HANDLERS} handlers"
        );
        self.handlers.insert(scheme, handler);
        Ok(())
    }

    /// Dispatches a URL to the appropriate handler if one is registered.
    pub fn handle(&self, url: &str) -> bool {
        self.try_handle(url).unwrap_or(false)
    }

    /// Validates and dispatches a URL while surfacing handler failures.
    pub fn try_handle(&self, url: &str) -> Result<bool> {
        anyhow::ensure!(
            url.len() <= MAX_DEEP_LINK_URL_BYTES,
            "deep-link URL cannot exceed {MAX_DEEP_LINK_URL_BYTES} bytes"
        );
        let Some((scheme, _)) = url.split_once(':') else {
            return Ok(false);
        };
        validate_deep_link_scheme(scheme)?;
        let scheme = scheme.to_ascii_lowercase();
        if let Some(handler) = self.handlers.get(&scheme) {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.handle(url)))
                .map_err(|_| anyhow!("deep-link handler panicked"))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Number of registered URL schemes.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Whether no URL schemes are registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

fn validate_deep_link_scheme(scheme: &str) -> Result<()> {
    anyhow::ensure!(!scheme.is_empty(), "deep-link scheme cannot be empty");
    anyhow::ensure!(
        scheme.len() <= MAX_DEEP_LINK_SCHEME_BYTES,
        "deep-link scheme cannot exceed {MAX_DEEP_LINK_SCHEME_BYTES} bytes"
    );
    let mut chars = scheme.chars();
    anyhow::ensure!(
        chars
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic()),
        "deep-link scheme must start with an ASCII letter"
    );
    anyhow::ensure!(
        chars
            .all(|character| character.is_ascii_alphanumeric()
                || matches!(character, '+' | '-' | '.')),
        "deep-link scheme contains invalid characters"
    );
    Ok(())
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

    /// Registers a deep link handler after validating its URL scheme.
    pub fn register_deep_link_handler_checked(
        &mut self,
        handler: Box<dyn DeepLinkHandler>,
    ) -> Result<()> {
        self.deep_link_registry.register_checked(handler)
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
        let _ = self.try_trigger();
    }

    /// Triggers the callback while containing callback panics.
    pub fn try_trigger(&mut self) -> Result<bool> {
        if let Some(ref mut callback) = self.on_reopen {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback))
                .map_err(|_| anyhow!("reopen callback panicked"))?;
            Ok(true)
        } else {
            Ok(false)
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
    fn test_checked_settings_store_validates_path() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct BuiltSettings {
            enabled: bool,
        }

        assert!(SettingsStore::<BuiltSettings>::new_checked("").is_err());
        assert!(
            SettingsStore::<BuiltSettings>::builder("")
                .validate()
                .is_err()
        );
        assert!(
            SettingsStore::<BuiltSettings>::builder("settings\n.json")
                .validate()
                .is_err()
        );

        let dir =
            std::env::temp_dir().join(format!("kael_settings_dir_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            SettingsStore::<BuiltSettings>::builder(&dir)
                .validate()
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_store_rejects_unsafe_files_versions_and_migrations() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct TestSettings {
            enabled: bool,
        }

        struct PanickingMigration;
        impl SettingsMigration for PanickingMigration {
            fn target_version(&self) -> u32 {
                1
            }
            fn migrate(&self, _value: &mut serde_json::Value) -> Result<()> {
                panic!("migration failed")
            }
        }

        let dir = std::env::temp_dir().join(format!(
            "kael-settings-safety-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        std::fs::write(
            &path,
            format!(
                "{{\"enabled\":true,\"_settings_version\":{}}}",
                u64::from(u32::MAX) + 1
            ),
        )
        .unwrap();
        assert!(SettingsStore::<TestSettings>::load(&path, &[]).is_err());

        std::fs::write(&path, r#"{"enabled":true}"#).unwrap();
        assert!(
            SettingsStore::<TestSettings>::load(&path, &[Box::new(PanickingMigration)]).is_err()
        );

        let oversized = std::fs::File::create(&path).unwrap();
        oversized.set_len(MAX_SETTINGS_BYTES as u64 + 1).unwrap();
        assert!(SettingsStore::<TestSettings>::load(&path, &[]).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = dir.join("target.json");
            std::fs::write(&target, r#"{"enabled":true}"#).unwrap();
            let link = dir.join("linked.json");
            symlink(&target, &link).unwrap();
            assert!(SettingsStore::<TestSettings>::load(&link, &[]).is_err());
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_updates_roll_back_and_saved_files_are_private() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct TestSettings {
            value: u32,
        }

        let dir = std::env::temp_dir().join(format!(
            "kael-settings-update-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let mut store = SettingsStore::<TestSettings>::new(&path);
        store.data_mut().value = 7;
        store.save().unwrap();

        assert!(
            store
                .update(|settings| {
                    settings.value = 42;
                    panic!("update failed")
                })
                .is_err()
        );
        assert_eq!(store.data().value, 7);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(std::fs::read_dir(&dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));

        let mut invalid_store = SettingsStore::<TestSettings>::new(&dir);
        invalid_store.data_mut().value = 9;
        assert!(
            invalid_store
                .update(|settings| settings.value = 10)
                .is_err()
        );
        assert_eq!(invalid_store.data().value, 9);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_checked_settings_store_validates_migrations() {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        struct BuiltSettings {
            enabled: bool,
        }

        struct Migration(u32);
        impl SettingsMigration for Migration {
            fn target_version(&self) -> u32 {
                self.0
            }

            fn migrate(&self, _value: &mut serde_json::Value) -> Result<()> {
                Ok(())
            }
        }

        let temp =
            std::env::temp_dir().join(format!("kael_checked_settings_test_{}", std::process::id()));

        assert!(
            SettingsStore::<BuiltSettings>::builder(&temp)
                .migration(Migration(0))
                .validate()
                .is_err()
        );
        assert!(
            SettingsStore::<BuiltSettings>::builder(&temp)
                .migration(Migration(1))
                .migration(Migration(1))
                .validate()
                .is_err()
        );

        let store = SettingsStore::<BuiltSettings>::builder(&temp)
            .migration(Migration(1))
            .load_checked()
            .unwrap();
        assert_eq!(store.path(), temp.as_path());

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
    fn test_command_registry_checked_registration() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let triggered = Arc::new(AtomicBool::new(false));
        let mut registry = CommandRegistry::new();
        registry
            .register_action_checked("editor.save", "Save", {
                let triggered = Arc::clone(&triggered);
                move || {
                    triggered.store(true, Ordering::Relaxed);
                }
            })
            .unwrap();

        assert!(registry.contains("editor.save"));
        registry.execute("editor.save").unwrap();
        assert!(triggered.load(Ordering::Relaxed));
        assert!(
            registry
                .register_action_checked("editor.save", "Save Again", || {})
                .is_err()
        );
    }

    #[test]
    fn command_registry_contains_handler_and_metadata_panics() {
        struct PanickingMetadata;
        impl AppCommand for PanickingMetadata {
            fn id(&self) -> &str {
                panic!("bad metadata")
            }
            fn name(&self) -> &str {
                "Broken"
            }
            fn execute(&self) {}
        }

        let mut registry = CommandRegistry::new();
        assert!(
            registry
                .register_checked(Box::new(PanickingMetadata))
                .is_err()
        );
        assert!(registry.is_empty());

        registry
            .register_action_checked("tools.panic", "Panic", || panic!("handler failed"))
            .unwrap();
        assert!(registry.execute("tools.panic").is_err());
        assert_eq!(registry.len(), 1);
        let debug = format!("{registry:?}");
        assert!(debug.contains("command_count"));
        assert!(!debug.contains("tools.panic"));
    }

    #[test]
    fn command_registry_bounds_growth_and_returns_stable_search_results() {
        let mut registry = CommandRegistry::new();
        registry.register_action("zebra.open", "Zebra", || {});
        registry.register_action("alpha.open", "Alpha", || {});
        registry.register_action("alpha.open", "Replacement", || {});

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.get("alpha.open").unwrap().name(), "Alpha");
        assert_eq!(registry.command_ids(), vec!["alpha.open", "zebra.open"]);
        assert_eq!(
            registry
                .search("open")
                .into_iter()
                .map(AppCommand::id)
                .collect::<Vec<_>>(),
            vec!["alpha.open", "zebra.open"]
        );
        assert!(registry.search("bad\nquery").is_empty());
        assert!(
            registry
                .search(&"x".repeat(MAX_COMMAND_SEARCH_BYTES + 1))
                .is_empty()
        );

        while registry.commands.len() < MAX_APP_COMMANDS {
            let index = registry.commands.len();
            registry.register_action(format!("command.{index}"), "Command", || {});
        }
        assert!(
            registry
                .register_action_checked("command.overflow", "Overflow", || {})
                .is_err()
        );
        assert_eq!(registry.len(), MAX_APP_COMMANDS);
    }

    #[test]
    fn test_command_registry_validates_generated_ids_and_names() {
        let mut registry = CommandRegistry::new();

        assert!(registry.register_action_checked("", "Save", || {}).is_err());
        assert!(
            registry
                .register_action_checked(" editor.save", "Save", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("editor save", "Save", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("editor\nsave", "Save", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("a".repeat(129), "Save", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("editor.save", "", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("editor.save", " Save", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("editor.save", "Save\nFile", || {})
                .is_err()
        );
        assert!(
            registry
                .register_action_checked("editor.save", "a".repeat(129), || {})
                .is_err()
        );

        let command = ClosureCommand::new("editor.open-recent", "Open Recent", || {});
        assert!(command.validate().is_ok());
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
        assert_eq!(undo.max_depth(), 10);
        assert_eq!(undo.undo_count(), 0);
        assert_eq!(undo.redo_count(), 0);
        assert_eq!(undo.transaction_change_count(), 0);
        assert_eq!(
            undo.to_text(),
            "undo redo: undo 0, redo 0, total 0, max-depth 10, at-max false, transaction false, transaction-changes 0"
        );

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
        assert_eq!(undo.undo_count(), 1);
        assert_eq!(undo.redo_count(), 1);
        assert_eq!(
            undo.to_text(),
            "undo redo: undo 1, redo 1, total 2, max-depth 10, at-max false, transaction false, transaction-changes 0"
        );
        assert!(!undo.to_text().contains("add 5"));
        assert!(!undo.to_text().contains("add 3"));

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
        assert_eq!(undo.undo_count(), 2);
        assert_eq!(undo.redo_count(), 0);
        assert!(undo.is_at_max_depth());
        assert_eq!(
            undo.to_text(),
            "undo redo: undo 2, redo 0, total 2, max-depth 2, at-max true, transaction false, transaction-changes 0"
        );
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
    fn test_undo_redo_checked_transactions_validate_state_and_description() {
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
        assert!(undo.end_transaction_checked().is_err());
        assert!(undo.begin_transaction_checked("").is_err());
        assert!(undo.begin_transaction_checked(" edit").is_err());
        assert!(undo.begin_transaction_checked("edit\nselection").is_err());
        assert!(undo.begin_transaction_checked("a".repeat(129)).is_err());

        undo.begin_transaction_checked("edit selection").unwrap();
        assert!(undo.has_open_transaction());
        assert_eq!(undo.transaction_change_count(), 0);
        assert_eq!(
            undo.to_text(),
            "undo redo: undo 0, redo 0, total 0, max-depth 10, at-max false, transaction true, transaction-changes 0"
        );
        assert!(undo.begin_transaction_checked("nested").is_err());
        assert!(!undo.end_transaction_checked().unwrap());
        assert!(!undo.has_open_transaction());

        undo.begin_transaction_checked("compound add").unwrap();
        undo.push(Box::new(AddChange {
            value: value.clone(),
            delta: 2,
        }));
        assert_eq!(undo.transaction_change_count(), 1);
        assert_eq!(
            undo.to_text(),
            "undo redo: undo 0, redo 0, total 0, max-depth 10, at-max false, transaction true, transaction-changes 1"
        );
        assert!(!undo.to_text().contains("compound add"));
        assert!(!undo.to_text().contains("add"));
        undo.push(Box::new(AddChange {
            value: value.clone(),
            delta: 3,
        }));

        assert!(undo.end_transaction_checked().unwrap());
        assert_eq!(*value.lock().unwrap(), 5);
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
    fn test_undo_redo_checked_operations_preserve_history_on_failure() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug)]
        struct PanicChange {
            apply_calls: Arc<AtomicUsize>,
            panic_apply_at: Option<usize>,
            panic_revert: bool,
            panic_description: bool,
        }

        impl UndoableChange for PanicChange {
            fn apply(&mut self) {
                let call = self.apply_calls.fetch_add(1, Ordering::Relaxed);
                if self.panic_apply_at == Some(call) {
                    panic!("private apply panic");
                }
            }

            fn revert(&mut self) {
                if self.panic_revert {
                    panic!("private revert panic");
                }
            }

            fn description(&self) -> &str {
                assert!(!self.panic_description, "private description panic");
                "test change"
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        assert!(UndoRedoManager::new_checked(MAX_UNDO_DEPTH + 1).is_err());
        assert_eq!(
            UndoRedoManager::new(MAX_UNDO_DEPTH + 1).max_depth(),
            MAX_UNDO_DEPTH
        );

        let mut undo = UndoRedoManager::new(10);
        undo.push_checked(Box::new(PanicChange {
            apply_calls: Arc::new(AtomicUsize::new(0)),
            panic_apply_at: None,
            panic_revert: false,
            panic_description: false,
        }))
        .unwrap();
        undo.undo_checked().unwrap();
        assert!(undo.can_redo());

        undo.begin_transaction_checked("empty transaction").unwrap();
        assert!(!undo.end_transaction_checked().unwrap());
        assert!(undo.can_redo());

        assert!(
            undo.push_checked(Box::new(PanicChange {
                apply_calls: Arc::new(AtomicUsize::new(0)),
                panic_apply_at: Some(0),
                panic_revert: false,
                panic_description: false,
            }))
            .is_err()
        );
        assert_eq!(undo.len(), 1);
        assert_eq!(undo.undo_count(), 0);
        assert!(undo.can_redo());

        assert!(
            undo.push_checked(Box::new(PanicChange {
                apply_calls: Arc::new(AtomicUsize::new(0)),
                panic_apply_at: None,
                panic_revert: false,
                panic_description: true,
            }))
            .is_err()
        );
        assert_eq!(undo.len(), 1);
        assert!(undo.can_redo());

        let mut revert_panics = UndoRedoManager::new(10);
        revert_panics
            .push_checked(Box::new(PanicChange {
                apply_calls: Arc::new(AtomicUsize::new(0)),
                panic_apply_at: None,
                panic_revert: true,
                panic_description: false,
            }))
            .unwrap();
        assert!(revert_panics.undo_checked().is_err());
        assert_eq!(revert_panics.undo_count(), 1);
        assert!(revert_panics.can_undo());

        let mut redo_panics = UndoRedoManager::new(10);
        redo_panics
            .push_checked(Box::new(PanicChange {
                apply_calls: Arc::new(AtomicUsize::new(0)),
                panic_apply_at: Some(1),
                panic_revert: false,
                panic_description: false,
            }))
            .unwrap();
        redo_panics.undo_checked().unwrap();
        assert!(redo_panics.redo_checked().is_err());
        assert_eq!(redo_panics.undo_count(), 0);
        assert!(redo_panics.can_redo());
    }

    #[test]
    fn test_clear_for_source_contains_panicking_source_callbacks() {
        #[derive(Debug)]
        struct SourceChange {
            source_id: Option<FocusId>,
            panic_source: bool,
        }

        impl UndoableChange for SourceChange {
            fn apply(&mut self) {}

            fn revert(&mut self) {}

            fn description(&self) -> &str {
                "source change"
            }

            fn source_id(&self) -> Option<FocusId> {
                assert!(!self.panic_source, "private source panic");
                self.source_id
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let mut focus_ids = SlotMap::<FocusId, ()>::with_key();
        let target = focus_ids.insert(());
        let other = focus_ids.insert(());
        let mut undo = UndoRedoManager::new(10);
        for change in [
            SourceChange {
                source_id: None,
                panic_source: true,
            },
            SourceChange {
                source_id: Some(target),
                panic_source: false,
            },
            SourceChange {
                source_id: Some(other),
                panic_source: false,
            },
        ] {
            undo.push_checked(Box::new(change)).unwrap();
        }

        assert!(undo.clear_for_source(target));
        assert_eq!(undo.len(), 2);
        assert_eq!(undo.undo_count(), 2);
        assert_eq!(
            undo.undo_descriptions(),
            vec!["source change", "source change"]
        );
        assert!(undo.has_undo_for_source(other));
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

        assert!(registry.handle("MYAPP:open/file"));
        assert!(triggered.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!registry.handle("unknown://test"));

        struct PanickingHandler;
        impl DeepLinkHandler for PanickingHandler {
            fn scheme(&self) -> &str {
                "panic-app"
            }

            fn handle(&self, _url: &str) {
                panic!("private deep-link panic");
            }
        }
        registry.register(Box::new(PanickingHandler));
        assert_eq!(
            registry
                .try_handle("panic-app://private")
                .unwrap_err()
                .to_string(),
            "deep-link handler panicked"
        );
        assert!(!registry.handle("panic-app://private"));
        assert!(registry.try_handle(&"x".repeat(16 * 1024 + 1)).is_err());
    }

    #[test]
    fn test_deep_link_registry_validates_metadata_duplicates_and_capacity() {
        #[derive(Debug)]
        struct TestHandler {
            scheme: String,
            panic_scheme: bool,
        }

        impl DeepLinkHandler for TestHandler {
            fn scheme(&self) -> &str {
                assert!(!self.panic_scheme, "private scheme panic");
                &self.scheme
            }

            fn handle(&self, _url: &str) {}
        }

        let mut registry = DeepLinkRegistry::new();
        assert!(registry.is_empty());
        for scheme in ["", "1app", "bad scheme", "app/route"] {
            assert!(
                registry
                    .register_checked(Box::new(TestHandler {
                        scheme: scheme.to_string(),
                        panic_scheme: false,
                    }))
                    .is_err()
            );
        }
        assert!(
            registry
                .register_checked(Box::new(TestHandler {
                    scheme: "ignored".to_string(),
                    panic_scheme: true,
                }))
                .is_err()
        );
        assert!(registry.is_empty());

        registry
            .register_checked(Box::new(TestHandler {
                scheme: "MyApp".to_string(),
                panic_scheme: false,
            }))
            .unwrap();
        assert_eq!(registry.len(), 1);
        assert!(
            registry
                .register_checked(Box::new(TestHandler {
                    scheme: "myapp".to_string(),
                    panic_scheme: false,
                }))
                .is_err()
        );
        assert_eq!(registry.len(), 1);
        assert!(registry.try_handle("bad scheme://private").is_err());

        for index in 1..MAX_DEEP_LINK_HANDLERS {
            registry
                .register_checked(Box::new(TestHandler {
                    scheme: format!("app{index}"),
                    panic_scheme: false,
                }))
                .unwrap();
        }
        assert_eq!(registry.len(), MAX_DEEP_LINK_HANDLERS);
        assert!(
            registry
                .register_checked(Box::new(TestHandler {
                    scheme: "overflow".to_string(),
                    panic_scheme: false,
                }))
                .is_err()
        );
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
        assert!(handler.try_trigger().unwrap());
        assert!(triggered.load(std::sync::atomic::Ordering::Relaxed));

        let mut empty = ReopenHandler::new();
        assert!(!empty.try_trigger().unwrap());

        let mut panicking = ReopenHandler::new().on_reopen(|| panic!("private reopen panic"));
        assert_eq!(
            panicking.try_trigger().unwrap_err().to_string(),
            "reopen callback panicked"
        );
        panicking.trigger();
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
