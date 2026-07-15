//! Plugin and extension architecture for GPUI.
//!
//! This module defines the plugin manifest format, capability model for
//! extensions, and the extension host contract. Plugins run in isolated
//! processes (building on the process-isolation model) and communicate with
//! the main application via typed IPC.

use std::{
    collections::{HashMap, HashSet},
    io::Read as _,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    process_model::{HelperProcessExecutionPlan, HelperProcessLaunch, HelperProcessLaunchBuilder},
    security::{Capability, IpcSchema, PermissionKind, PluginPermissionManifest},
};

const MAX_PLUGIN_MANIFEST_BYTES: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// Plugin Manifest
// ---------------------------------------------------------------------------

/// A versioned plugin manifest that describes the plugin, its capabilities,
/// and its contribution points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin identifier (reverse-DNS recommended).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Plugin version (semver).
    pub version: String,
    /// The API version this plugin targets.
    pub api_version: String,
    /// A short description.
    pub description: Option<String>,
    /// The plugin author.
    pub author: Option<String>,
    /// Entry point executable or WASM module path.
    pub entry_point: String,
    /// Execution model for the plugin.
    pub execution_model: ExecutionModel,
    /// Capabilities requested by the plugin.
    pub capabilities: Vec<Capability>,
    /// Command-line arguments passed to the entry point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// UI contribution points.
    pub contributions: Contributions,
}

/// How the plugin code is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionModel {
    /// Run as a separate native process.
    ExternalProcess,
    /// Run in a sandboxed WASM runtime.
    Wasm,
}

impl ExecutionModel {
    /// Stable execution model key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::ExternalProcess => "external-process",
            Self::Wasm => "wasm",
        }
    }
}

impl PluginManifest {
    /// Validate the manifest for well-formedness.
    pub fn validate(&self) -> Result<()> {
        validate_plugin_id(&self.id, "plugin id")?;
        anyhow::ensure!(
            !self.id.contains('/'),
            "plugin id cannot contain path separators"
        );
        validate_plugin_label(&self.name, "plugin name")?;
        validate_plugin_version(&self.version, "plugin version")?;
        validate_plugin_version(&self.api_version, "plugin api version")?;
        validate_plugin_path_text(&self.entry_point, "plugin entry point")?;
        if let Some(description) = &self.description {
            validate_optional_plugin_text(description, "plugin description")?;
        }
        if let Some(author) = &self.author {
            validate_optional_plugin_text(author, "plugin author")?;
        }
        anyhow::ensure!(
            self.capabilities.len() <= 64,
            "plugin cannot request more than 64 capabilities"
        );
        let mut capabilities = HashSet::new();
        anyhow::ensure!(
            self.capabilities
                .iter()
                .all(|capability| capabilities.insert(capability)),
            "plugin cannot request the same capability more than once"
        );
        anyhow::ensure!(
            self.args.len() <= 1_024,
            "plugin cannot include more than 1024 arguments"
        );
        for arg in &self.args {
            validate_optional_plugin_text(arg, "plugin argument")?;
        }
        self.contributions.validate()?;
        Ok(())
    }

    /// Return all requested high-risk capabilities.
    pub fn high_risk_capabilities(&self) -> Vec<Capability> {
        self.capabilities
            .iter()
            .filter(|capability| capability.is_high_risk())
            .cloned()
            .collect()
    }

    /// Byte length of the plugin id without exposing it.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Byte length of the plugin name without exposing it.
    pub fn name_len_bytes(&self) -> usize {
        self.name.len()
    }

    /// Byte length of the plugin version without exposing it.
    pub fn version_len_bytes(&self) -> usize {
        self.version.len()
    }

    /// Byte length of the API version without exposing it.
    pub fn api_version_len_bytes(&self) -> usize {
        self.api_version.len()
    }

    /// Byte length of the entry point path text without exposing it.
    pub fn entry_point_len_bytes(&self) -> usize {
        self.entry_point.len()
    }

    /// Returns true when the manifest includes a description.
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// Byte length of the description without exposing it.
    pub fn description_len_bytes(&self) -> usize {
        self.description
            .as_ref()
            .map_or(0, |description| description.len())
    }

    /// Returns true when the manifest includes an author.
    pub fn has_author(&self) -> bool {
        self.author.is_some()
    }

    /// Byte length of the author without exposing it.
    pub fn author_len_bytes(&self) -> usize {
        self.author.as_ref().map_or(0, |author| author.len())
    }

    /// Number of requested capabilities.
    pub fn capability_count(&self) -> usize {
        self.capabilities.len()
    }

    /// Number of requested high-risk capabilities.
    pub fn high_risk_capability_count(&self) -> usize {
        self.capabilities
            .iter()
            .filter(|capability| capability.is_high_risk())
            .count()
    }

    /// Number of command-line arguments.
    pub fn arg_count(&self) -> usize {
        self.args.len()
    }

    /// Content-safe manifest summary.
    pub fn to_text(&self) -> String {
        format!(
            "plugin_manifest(id_len_bytes={}, name_len_bytes={}, version_len_bytes={}, api_version_len_bytes={}, entry_point_len_bytes={}, execution_model={}, capabilities={}, high_risk_capabilities={}, args={}, has_description={}, description_len_bytes={}, has_author={}, author_len_bytes={}, contributions={})",
            self.id_len_bytes(),
            self.name_len_bytes(),
            self.version_len_bytes(),
            self.api_version_len_bytes(),
            self.entry_point_len_bytes(),
            self.execution_model.to_text(),
            self.capability_count(),
            self.high_risk_capability_count(),
            self.arg_count(),
            self.has_description(),
            self.description_len_bytes(),
            self.has_author(),
            self.author_len_bytes(),
            self.contributions.to_text()
        )
    }

    /// Parse a plugin manifest from a JSON string.
    pub fn from_json(json: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Parse a plugin manifest from a TOML string.
    pub fn from_toml(toml: &str) -> Result<Self> {
        let manifest: Self = toml::from_str(toml)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load a plugin manifest from a filesystem path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let metadata = std::fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.file_type().is_file(),
            "plugin manifest must be a regular file"
        );
        anyhow::ensure!(
            metadata.len() <= MAX_PLUGIN_MANIFEST_BYTES,
            "plugin manifest exceeds {MAX_PLUGIN_MANIFEST_BYTES} byte limit"
        );
        let mut contents = String::new();
        std::fs::File::open(path)?
            .take(MAX_PLUGIN_MANIFEST_BYTES + 1)
            .read_to_string(&mut contents)?;
        anyhow::ensure!(
            contents.len() as u64 <= MAX_PLUGIN_MANIFEST_BYTES,
            "plugin manifest exceeds {MAX_PLUGIN_MANIFEST_BYTES} byte limit"
        );
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Self::from_json(&contents),
            Some("toml") => Self::from_toml(&contents),
            Some(other) => anyhow::bail!("unsupported plugin manifest format: {}", other),
            None => anyhow::bail!("plugin manifest file must have an extension"),
        }
    }

    /// Create a builder for a plugin manifest.
    pub fn builder(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        api_version: impl Into<String>,
        entry_point: impl Into<String>,
        execution_model: ExecutionModel,
    ) -> PluginManifestBuilder {
        PluginManifestBuilder::new(id, name, version, api_version, entry_point, execution_model)
    }
}

/// Builder for [`PluginManifest`].
pub struct PluginManifestBuilder {
    manifest: PluginManifest,
}

impl PluginManifestBuilder {
    /// Create a new manifest builder.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        api_version: impl Into<String>,
        entry_point: impl Into<String>,
        execution_model: ExecutionModel,
    ) -> Self {
        Self {
            manifest: PluginManifest {
                id: id.into(),
                name: name.into(),
                version: version.into(),
                api_version: api_version.into(),
                description: None,
                author: None,
                entry_point: entry_point.into(),
                execution_model,
                capabilities: Vec::new(),
                args: Vec::new(),
                contributions: Contributions::default(),
            },
        }
    }

    /// Set the manifest description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.manifest.description = Some(description.into());
        self
    }

    /// Set the manifest author.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.manifest.author = Some(author.into());
        self
    }

    /// Add a requested capability.
    pub fn capability(mut self, capability: Capability) -> Self {
        self.manifest.capabilities.push(capability);
        self
    }

    /// Add a command-line argument for the entry point.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.manifest.args.push(arg.into());
        self
    }

    /// Add a command contribution.
    pub fn command(mut self, command: ContributedCommand) -> Self {
        self.manifest.contributions.commands.push(command);
        self
    }

    /// Add a menu-item contribution.
    pub fn menu_item(mut self, menu_item: ContributedMenuItem) -> Self {
        self.manifest.contributions.menu_items.push(menu_item);
        self
    }

    /// Add a panel contribution.
    pub fn panel(mut self, panel: ContributedPanel) -> Self {
        self.manifest.contributions.panels.push(panel);
        self
    }

    /// Set the settings schema contribution.
    pub fn settings_schema(mut self, schema: serde_json::Value) -> Self {
        self.manifest.contributions.settings_schema = Some(schema);
        self
    }

    /// Replace the contribution set.
    pub fn contributions(mut self, contributions: Contributions) -> Self {
        self.manifest.contributions = contributions;
        self
    }

    /// Finalize the manifest.
    pub fn build(self) -> Result<PluginManifest> {
        self.manifest.validate()?;
        Ok(self.manifest)
    }
}

// ---------------------------------------------------------------------------
// Contribution Points
// ---------------------------------------------------------------------------

/// UI and behavior contributions that a plugin can register.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Contributions {
    /// Commands contributed to the command palette.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<ContributedCommand>,
    /// Menu items contributed to application menus.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu_items: Vec<ContributedMenuItem>,
    /// Panels contributed to the workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<ContributedPanel>,
    /// Settings schema contributed by the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_schema: Option<serde_json::Value>,
}

/// A command contributed by a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributedCommand {
    /// Unique command identifier within the plugin.
    pub id: String,
    /// Display name for the command palette.
    pub title: String,
    /// Optional keyboard shortcut.
    pub keybinding: Option<String>,
}

/// A menu item contributed by a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributedMenuItem {
    /// The menu to contribute to (e.g., "file", "edit", "view").
    pub target_menu: String,
    /// Display label.
    pub label: String,
    /// Command to invoke when activated.
    pub command_id: String,
}

/// A panel contributed by a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributedPanel {
    /// Unique panel identifier.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Default dock position.
    pub default_position: PanelPosition,
}

/// Dock position for a contributed panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelPosition {
    /// Dock to the left side of the workspace.
    Left,
    /// Dock to the right side of the workspace.
    Right,
    /// Dock to the bottom of the workspace.
    Bottom,
    /// Float as a separate window or overlay.
    Floating,
}

impl Contributions {
    /// Validate contributed commands, menus, panels, and settings schema.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.commands.len() <= 4_096,
            "plugin cannot contribute more than 4096 commands"
        );
        anyhow::ensure!(
            self.menu_items.len() <= 4_096,
            "plugin cannot contribute more than 4096 menu items"
        );
        anyhow::ensure!(
            self.panels.len() <= 4_096,
            "plugin cannot contribute more than 4096 panels"
        );
        let mut command_ids = HashSet::new();
        for command in &self.commands {
            command.validate()?;
            anyhow::ensure!(
                command_ids.insert(command.id.as_str()),
                "plugin command id is duplicated: {}",
                command.id
            );
        }

        for menu_item in &self.menu_items {
            menu_item.validate()?;
            anyhow::ensure!(
                command_ids.contains(menu_item.command_id.as_str()),
                "plugin menu item references unknown command id: {}",
                menu_item.command_id
            );
        }

        let mut panel_ids = HashSet::new();
        for panel in &self.panels {
            panel.validate()?;
            anyhow::ensure!(
                panel_ids.insert(panel.id.as_str()),
                "plugin panel id is duplicated: {}",
                panel.id
            );
        }

        if let Some(schema) = &self.settings_schema {
            anyhow::ensure!(
                schema.is_object(),
                "plugin settings schema must be a JSON object"
            );
            anyhow::ensure!(
                serde_json::to_vec(schema)?.len() <= MAX_PLUGIN_MANIFEST_BYTES as usize,
                "plugin settings schema exceeds {MAX_PLUGIN_MANIFEST_BYTES} byte limit"
            );
        }

        Ok(())
    }

    /// Number of command contributions.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Number of menu-item contributions.
    pub fn menu_item_count(&self) -> usize {
        self.menu_items.len()
    }

    /// Number of panel contributions.
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// Returns true when a settings schema is contributed.
    pub fn has_settings_schema(&self) -> bool {
        self.settings_schema.is_some()
    }

    /// Content-safe contribution summary.
    pub fn to_text(&self) -> String {
        format!(
            "plugin_contributions(commands={}, menu_items={}, panels={}, has_settings_schema={})",
            self.command_count(),
            self.menu_item_count(),
            self.panel_count(),
            self.has_settings_schema()
        )
    }
}

impl ContributedCommand {
    /// Validate a command contribution.
    pub fn validate(&self) -> Result<()> {
        validate_plugin_id(&self.id, "plugin command id")?;
        validate_plugin_label(&self.title, "plugin command title")?;
        if let Some(keybinding) = &self.keybinding {
            validate_optional_plugin_text(keybinding, "plugin command keybinding")?;
        }
        Ok(())
    }

    /// Byte length of the command id without exposing it.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Byte length of the title without exposing it.
    pub fn title_len_bytes(&self) -> usize {
        self.title.len()
    }

    /// Returns true when a keybinding is configured.
    pub fn has_keybinding(&self) -> bool {
        self.keybinding.is_some()
    }

    /// Byte length of the keybinding without exposing it.
    pub fn keybinding_len_bytes(&self) -> usize {
        self.keybinding
            .as_ref()
            .map_or(0, |keybinding| keybinding.len())
    }

    /// Content-safe command contribution summary.
    pub fn to_text(&self) -> String {
        format!(
            "contributed_command(id_len_bytes={}, title_len_bytes={}, has_keybinding={}, keybinding_len_bytes={})",
            self.id_len_bytes(),
            self.title_len_bytes(),
            self.has_keybinding(),
            self.keybinding_len_bytes()
        )
    }
}

impl ContributedMenuItem {
    /// Validate a menu item contribution.
    pub fn validate(&self) -> Result<()> {
        validate_plugin_id(&self.target_menu, "plugin menu target")?;
        validate_plugin_label(&self.label, "plugin menu item label")?;
        validate_plugin_id(&self.command_id, "plugin menu command id")?;
        Ok(())
    }

    /// Byte length of the target menu id without exposing it.
    pub fn target_menu_len_bytes(&self) -> usize {
        self.target_menu.len()
    }

    /// Byte length of the label without exposing it.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Byte length of the command id without exposing it.
    pub fn command_id_len_bytes(&self) -> usize {
        self.command_id.len()
    }

    /// Content-safe menu contribution summary.
    pub fn to_text(&self) -> String {
        format!(
            "contributed_menu_item(target_menu_len_bytes={}, label_len_bytes={}, command_id_len_bytes={})",
            self.target_menu_len_bytes(),
            self.label_len_bytes(),
            self.command_id_len_bytes()
        )
    }
}

impl ContributedPanel {
    /// Validate a panel contribution.
    pub fn validate(&self) -> Result<()> {
        validate_plugin_id(&self.id, "plugin panel id")?;
        validate_plugin_label(&self.title, "plugin panel title")?;
        Ok(())
    }

    /// Byte length of the panel id without exposing it.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Byte length of the panel title without exposing it.
    pub fn title_len_bytes(&self) -> usize {
        self.title.len()
    }

    /// Stable default dock position key.
    pub fn position_key(&self) -> &'static str {
        self.default_position.to_text()
    }

    /// Content-safe panel contribution summary.
    pub fn to_text(&self) -> String {
        format!(
            "contributed_panel(id_len_bytes={}, title_len_bytes={}, position={})",
            self.id_len_bytes(),
            self.title_len_bytes(),
            self.position_key()
        )
    }
}

impl PanelPosition {
    /// Stable panel position key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Floating => "floating",
        }
    }
}

fn validate_plugin_id(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.len() <= 128,
        "{label} cannot be longer than 128 bytes"
    );
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '-' | '_' | '/')),
        "{label} must contain only ASCII letters, numbers, '.', ':', '-', '_' or '/'"
    );
    Ok(())
}

fn validate_plugin_label(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.chars().count() <= 128,
        "{label} cannot be longer than 128 characters"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

fn validate_plugin_version(value: &str, label: &str) -> Result<()> {
    validate_plugin_label(value, label)?;
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+')),
        "{label} must be a portable version string"
    );
    Ok(())
}

fn validate_plugin_path_text(value: &str, label: &str) -> Result<()> {
    validate_plugin_label(value, label)?;
    anyhow::ensure!(
        !value.contains('\0'),
        "{label} cannot contain NUL characters"
    );
    Ok(())
}

fn validate_optional_plugin_text(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(
        value.chars().count() <= 512,
        "{label} cannot be longer than 512 characters"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Extension Host
// ---------------------------------------------------------------------------

/// Information about a loaded extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionInfo {
    /// The extension manifest.
    pub manifest: PluginManifest,
    /// Whether the extension is currently active.
    pub is_active: bool,
    /// Process identifier if running out-of-process.
    pub process_id: Option<crate::process_model::ProcessId>,
    /// Filesystem path the extension was loaded from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_path: Option<PathBuf>,
    /// Whether the extension was loaded in dev mode (not copied).
    #[serde(default)]
    pub dev_mode: bool,
}

impl ExtensionInfo {
    /// Returns true when the extension has an associated running process.
    pub fn has_process(&self) -> bool {
        self.process_id.is_some()
    }

    /// Returns true when the extension has a load path.
    pub fn has_load_path(&self) -> bool {
        self.load_path.is_some()
    }

    /// Content-safe extension info summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_info(active={}, has_process={}, has_load_path={}, dev_mode={}, manifest={})",
            self.is_active,
            self.has_process(),
            self.has_load_path(),
            self.dev_mode,
            self.manifest.to_text()
        )
    }
}

/// The extension host manages plugin lifecycle and IPC.
pub struct ExtensionHost {
    extensions: HashMap<String, ExtensionInfo>,
}

impl ExtensionHost {
    /// Create a new extension host.
    pub fn new() -> Self {
        Self {
            extensions: HashMap::new(),
        }
    }

    /// Load a plugin manifest into the host.
    pub fn load_manifest(&mut self, manifest: PluginManifest) -> Result<()> {
        self.load_manifest_with_options(manifest, None, false)
    }

    /// Load a plugin manifest with runtime options.
    pub fn load_manifest_with_options(
        &mut self,
        manifest: PluginManifest,
        load_path: Option<PathBuf>,
        dev_mode: bool,
    ) -> Result<()> {
        manifest.validate()?;
        if self.extensions.contains_key(&manifest.id) {
            anyhow::bail!("extension already loaded: {}", manifest.id);
        }
        let info = ExtensionInfo {
            manifest,
            is_active: false,
            process_id: None,
            load_path,
            dev_mode,
        };
        self.extensions.insert(info.manifest.id.clone(), info);
        Ok(())
    }

    /// Load a plugin manifest from a filesystem path.
    pub fn load_manifest_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.load_manifest(PluginManifest::load(path)?)
    }

    /// Activate a loaded extension.
    pub fn activate(&mut self, id: &str) -> Result<()> {
        let info = self
            .extensions
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("extension not found: {}", id))?;
        info.is_active = true;
        Ok(())
    }

    /// Mark an extension as active and associate it with a process.
    pub fn attach_process(
        &mut self,
        id: &str,
        process_id: crate::process_model::ProcessId,
    ) -> Result<()> {
        let info = self
            .extensions
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("extension not found: {}", id))?;
        info.is_active = true;
        info.process_id = Some(process_id);
        Ok(())
    }

    /// Deactivate an extension.
    pub fn deactivate(&mut self, id: &str) -> Result<()> {
        let info = self
            .extensions
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("extension not found: {}", id))?;
        info.is_active = false;
        info.process_id = None;
        Ok(())
    }

    /// Unload an extension.
    pub fn unload(&mut self, id: &str) -> Result<()> {
        if let Some(info) = self.extensions.get(id) {
            if info.is_active {
                anyhow::bail!("cannot unload active extension: {}", id);
            }
        }
        self.extensions.remove(id);
        Ok(())
    }

    /// Get information about a loaded extension.
    pub fn get(&self, id: &str) -> Option<&ExtensionInfo> {
        self.extensions.get(id)
    }

    /// Return all loaded extensions.
    pub fn all(&self) -> Vec<&ExtensionInfo> {
        self.extensions.values().collect()
    }

    /// Return all active extensions.
    pub fn active(&self) -> Vec<&ExtensionInfo> {
        self.extensions.values().filter(|e| e.is_active).collect()
    }

    /// Return the command contributions from all active extensions.
    pub fn active_commands(&self) -> Vec<&ContributedCommand> {
        self.active()
            .into_iter()
            .flat_map(|extension| extension.manifest.contributions.commands.iter())
            .collect()
    }

    /// Return the menu-item contributions from all active extensions.
    pub fn active_menu_items(&self) -> Vec<&ContributedMenuItem> {
        self.active()
            .into_iter()
            .flat_map(|extension| extension.manifest.contributions.menu_items.iter())
            .collect()
    }

    /// Return the panel contributions from all active extensions.
    pub fn active_panels(&self) -> Vec<&ContributedPanel> {
        self.active()
            .into_iter()
            .flat_map(|extension| extension.manifest.contributions.panels.iter())
            .collect()
    }

    /// Number of loaded extensions.
    pub fn loaded_count(&self) -> usize {
        self.extensions.len()
    }

    /// Number of active extensions.
    pub fn active_count(&self) -> usize {
        self.extensions
            .values()
            .filter(|extension| extension.is_active)
            .count()
    }

    /// Number of extensions loaded in dev mode.
    pub fn dev_mode_count(&self) -> usize {
        self.extensions
            .values()
            .filter(|extension| extension.dev_mode)
            .count()
    }

    /// Number of extensions with an attached process.
    pub fn process_count(&self) -> usize {
        self.extensions
            .values()
            .filter(|extension| extension.process_id.is_some())
            .count()
    }

    /// Content-safe extension host summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_host(loaded={}, active={}, dev_mode={}, processes={}, active_commands={}, active_menu_items={}, active_panels={})",
            self.loaded_count(),
            self.active_count(),
            self.dev_mode_count(),
            self.process_count(),
            self.active_commands().len(),
            self.active_menu_items().len(),
            self.active_panels().len()
        )
    }
}

impl Default for ExtensionHost {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Extension Manifest Format
// ---------------------------------------------------------------------------

/// A higher-level extension manifest that describes an extension with typed
/// contribution points, permissions, and activation events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// Unique extension identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Extension version (semver).
    pub version: String,
    /// A short description of what the extension does.
    pub description: String,
    /// The extension author.
    pub author: Option<String>,
    /// SPDX license identifier.
    pub license: Option<String>,
    /// Typed contribution points the extension provides.
    pub contribution_points: Vec<ContributionPoint>,
    /// Permissions required by the extension.
    pub permissions: Vec<String>,
    /// Events that trigger extension activation.
    pub activation_events: Vec<String>,
}

impl ExtensionManifest {
    /// Validate the extension manifest for well-formedness.
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            anyhow::bail!("extension manifest: id must not be empty");
        }
        if self.name.is_empty() {
            anyhow::bail!("extension manifest: name must not be empty");
        }
        if self.version.is_empty() {
            anyhow::bail!("extension manifest: version must not be empty");
        }
        if self.description.is_empty() {
            anyhow::bail!("extension manifest: description must not be empty");
        }
        Ok(())
    }

    /// Extract all command contribution points.
    pub fn commands(&self) -> Vec<(&str, &str)> {
        self.contribution_points
            .iter()
            .filter_map(|cp| match cp {
                ContributionPoint::Command { id, title, .. } => Some((id.as_str(), title.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Extract all panel contribution points.
    pub fn panels(&self) -> Vec<(&str, &str)> {
        self.contribution_points
            .iter()
            .filter_map(|cp| match cp {
                ContributionPoint::Panel { id, title, .. } => Some((id.as_str(), title.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Extract all theme contribution points.
    pub fn themes(&self) -> Vec<(&str, &str)> {
        self.contribution_points
            .iter()
            .filter_map(|cp| match cp {
                ContributionPoint::Theme { id, label } => Some((id.as_str(), label.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Extract file-type contribution points matching a given file extension.
    pub fn handles_file_extension(&self, ext: &str) -> bool {
        self.contribution_points.iter().any(|cp| match cp {
            ContributionPoint::FileType { extensions, .. } => extensions.iter().any(|e| e == ext),
            _ => false,
        })
    }

    /// Byte length of the extension id without exposing it.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Byte length of the extension name without exposing it.
    pub fn name_len_bytes(&self) -> usize {
        self.name.len()
    }

    /// Byte length of the extension version without exposing it.
    pub fn version_len_bytes(&self) -> usize {
        self.version.len()
    }

    /// Byte length of the description without exposing it.
    pub fn description_len_bytes(&self) -> usize {
        self.description.len()
    }

    /// Returns true when the manifest includes an author.
    pub fn has_author(&self) -> bool {
        self.author.is_some()
    }

    /// Returns true when the manifest includes a license.
    pub fn has_license(&self) -> bool {
        self.license.is_some()
    }

    /// Number of contribution points.
    pub fn contribution_point_count(&self) -> usize {
        self.contribution_points.len()
    }

    /// Content-safe extension manifest summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_manifest(id_len_bytes={}, name_len_bytes={}, version_len_bytes={}, description_len_bytes={}, has_author={}, has_license={}, contribution_points={}, permissions={}, activation_events={}, commands={}, panels={}, themes={})",
            self.id_len_bytes(),
            self.name_len_bytes(),
            self.version_len_bytes(),
            self.description_len_bytes(),
            self.has_author(),
            self.has_license(),
            self.contribution_point_count(),
            self.permissions.len(),
            self.activation_events.len(),
            self.commands().len(),
            self.panels().len(),
            self.themes().len()
        )
    }
}

// ---------------------------------------------------------------------------
// Contribution Points (typed enum)
// ---------------------------------------------------------------------------

/// A typed contribution point that an extension can declare.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContributionPoint {
    /// A command contribution for the command palette.
    Command {
        /// Unique command identifier.
        id: String,
        /// Display title.
        title: String,
        /// Optional keyboard shortcut.
        keybinding: Option<String>,
    },
    /// A menu contribution.
    Menu {
        /// Target menu location (e.g. "file", "edit").
        location: String,
        /// Menu items to add.
        items: Vec<PluginMenuItem>,
    },
    /// A panel contribution.
    Panel {
        /// Unique panel identifier.
        id: String,
        /// Display title.
        title: String,
        /// Optional icon path or name.
        icon: Option<String>,
    },
    /// A settings contribution.
    Setting {
        /// Settings key path.
        key: String,
        /// Default value for the setting.
        default_value: serde_json::Value,
        /// Human-readable description of the setting.
        description: String,
    },
    /// A file-type handler contribution.
    FileType {
        /// File extensions handled (without leading dot).
        extensions: Vec<String>,
        /// Language identifier for syntax highlighting.
        language_id: String,
    },
    /// A theme contribution.
    Theme {
        /// Unique theme identifier.
        id: String,
        /// Display label.
        label: String,
    },
    /// A keybinding contribution.
    Keybinding {
        /// Command to invoke.
        command: String,
        /// Key combination string.
        key: String,
        /// Optional context condition.
        when: Option<String>,
    },
}

/// A single item within a menu contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginMenuItem {
    /// Command to invoke when the menu item is activated.
    pub command: String,
    /// Display title.
    pub title: String,
    /// Optional group within the menu for separators.
    pub group: Option<String>,
}

// ---------------------------------------------------------------------------
// Extension Host Diagnostics
// ---------------------------------------------------------------------------

/// Lifecycle state of an extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionState {
    /// The extension is loaded but not yet activated.
    Inactive,
    /// The extension is in the process of activating.
    Activating,
    /// The extension is fully active.
    Active,
    /// The extension is in the process of deactivating.
    Deactivating,
    /// The extension encountered an error.
    Error(String),
    /// The extension process crashed.
    Crashed,
}

impl ExtensionState {
    /// Stable extension lifecycle state key.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Activating => "activating",
            Self::Active => "active",
            Self::Deactivating => "deactivating",
            Self::Error(_) => "error",
            Self::Crashed => "crashed",
        }
    }

    /// Returns true when the state carries an error message.
    pub fn has_error_message(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Byte length of the error message without exposing it.
    pub fn error_message_len_bytes(&self) -> usize {
        match self {
            Self::Error(message) => message.len(),
            _ => 0,
        }
    }
}

/// Diagnostic information for a running extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionDiagnostics {
    /// Extension identifier.
    pub id: String,
    /// Current lifecycle state.
    pub state: ExtensionState,
    /// Time taken to activate in milliseconds.
    pub activation_time_ms: Option<u64>,
    /// Approximate memory usage in bytes.
    pub memory_usage_bytes: Option<u64>,
    /// Running count of errors encountered.
    pub error_count: u32,
    /// Most recent error message, if any.
    pub last_error: Option<String>,
}

impl ExtensionDiagnostics {
    /// Create a new diagnostics record for the given extension.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            state: ExtensionState::Inactive,
            activation_time_ms: None,
            memory_usage_bytes: None,
            error_count: 0,
            last_error: None,
        }
    }

    /// Record an error, incrementing the counter and storing the message.
    pub fn record_error(&mut self, message: impl Into<String>) {
        let msg = message.into();
        self.error_count += 1;
        self.last_error = Some(msg);
    }

    /// Byte length of the extension id without exposing it.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns true when activation timing is available.
    pub fn has_activation_time(&self) -> bool {
        self.activation_time_ms.is_some()
    }

    /// Returns true when memory usage is available.
    pub fn has_memory_usage(&self) -> bool {
        self.memory_usage_bytes.is_some()
    }

    /// Returns true when a last error message is stored.
    pub fn has_last_error(&self) -> bool {
        self.last_error.is_some()
    }

    /// Byte length of the last error without exposing it.
    pub fn last_error_len_bytes(&self) -> usize {
        self.last_error.as_ref().map_or(0, |error| error.len())
    }

    /// Content-safe diagnostics summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_diagnostics(id_len_bytes={}, state={}, has_state_error_message={}, state_error_message_len_bytes={}, has_activation_time={}, has_memory_usage={}, error_count={}, has_last_error={}, last_error_len_bytes={})",
            self.id_len_bytes(),
            self.state.to_text(),
            self.state.has_error_message(),
            self.state.error_message_len_bytes(),
            self.has_activation_time(),
            self.has_memory_usage(),
            self.error_count,
            self.has_last_error(),
            self.last_error_len_bytes()
        )
    }
}

// ---------------------------------------------------------------------------
// Extension Registry
// ---------------------------------------------------------------------------

/// A registry that tracks extension manifests and their diagnostics.
pub struct ExtensionRegistry {
    extensions: HashMap<String, ExtensionManifest>,
    diagnostics: HashMap<String, ExtensionDiagnostics>,
}

impl ExtensionRegistry {
    /// Create an empty extension registry.
    pub fn new() -> Self {
        Self {
            extensions: HashMap::new(),
            diagnostics: HashMap::new(),
        }
    }

    /// Register a new extension manifest. Returns an error if the id is
    /// already registered or if the manifest is invalid.
    pub fn register(&mut self, manifest: ExtensionManifest) -> Result<()> {
        manifest.validate()?;
        if self.extensions.contains_key(&manifest.id) {
            anyhow::bail!("extension already registered: {}", manifest.id);
        }
        let diag = ExtensionDiagnostics::new(&manifest.id);
        self.diagnostics.insert(manifest.id.clone(), diag);
        self.extensions.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    /// Unregister an extension, returning its manifest.
    pub fn unregister(&mut self, id: &str) -> Result<ExtensionManifest> {
        self.diagnostics.remove(id);
        self.extensions
            .remove(id)
            .ok_or_else(|| anyhow::anyhow!("extension not found: {}", id))
    }

    /// Look up an extension manifest by id.
    pub fn get(&self, id: &str) -> Option<&ExtensionManifest> {
        self.extensions.get(id)
    }

    /// Return all registered extension manifests.
    pub fn list(&self) -> Vec<&ExtensionManifest> {
        self.extensions.values().collect()
    }

    /// Update the lifecycle state for an extension.
    pub fn update_diagnostics(&mut self, id: &str, state: ExtensionState) -> Result<()> {
        let diag = self
            .diagnostics
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("extension not found: {}", id))?;
        if let ExtensionState::Error(ref msg) = state {
            diag.record_error(msg.clone());
        }
        diag.state = state;
        Ok(())
    }

    /// Get diagnostic information for an extension.
    pub fn get_diagnostics(&self, id: &str) -> Option<&ExtensionDiagnostics> {
        self.diagnostics.get(id)
    }

    /// Collect all command contribution points across registered extensions.
    pub fn commands(&self) -> Vec<(&str, &str)> {
        self.extensions
            .values()
            .flat_map(|m| m.commands())
            .collect()
    }

    /// Collect all panel contribution points across registered extensions.
    pub fn panels(&self) -> Vec<(&str, &str)> {
        self.extensions.values().flat_map(|m| m.panels()).collect()
    }

    /// Collect all theme contribution points across registered extensions.
    pub fn themes(&self) -> Vec<(&str, &str)> {
        self.extensions.values().flat_map(|m| m.themes()).collect()
    }

    /// Return manifests for extensions that handle a given file extension.
    pub fn file_type_handlers(&self, extension: &str) -> Vec<&ExtensionManifest> {
        self.extensions
            .values()
            .filter(|m| m.handles_file_extension(extension))
            .collect()
    }

    /// Number of registered extensions.
    pub fn extension_count(&self) -> usize {
        self.extensions.len()
    }

    /// Number of diagnostics entries.
    pub fn diagnostics_count(&self) -> usize {
        self.diagnostics.len()
    }

    /// Total recorded extension errors.
    pub fn total_error_count(&self) -> u32 {
        self.diagnostics
            .values()
            .map(|diagnostics| diagnostics.error_count)
            .sum()
    }

    /// Number of extensions currently in an error-like state.
    pub fn unhealthy_count(&self) -> usize {
        self.diagnostics
            .values()
            .filter(|diagnostics| {
                matches!(
                    diagnostics.state,
                    ExtensionState::Error(_) | ExtensionState::Crashed
                )
            })
            .count()
    }

    /// Content-safe extension registry summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_registry(extensions={}, diagnostics={}, commands={}, panels={}, themes={}, total_errors={}, unhealthy={})",
            self.extension_count(),
            self.diagnostics_count(),
            self.commands().len(),
            self.panels().len(),
            self.themes().len(),
            self.total_error_count(),
            self.unhealthy_count()
        )
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Extension Crash / Restart
// ---------------------------------------------------------------------------

/// Policy governing automatic restarts after extension crashes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrashPolicy {
    /// Maximum number of restarts before the extension is disabled.
    pub max_restarts: u32,
    /// Base delay before the first restart in milliseconds.
    pub restart_delay_ms: u64,
    /// Multiplicative factor applied to the delay after each successive crash.
    pub backoff_factor: f64,
}

impl Default for CrashPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 3,
            restart_delay_ms: 1000,
            backoff_factor: 2.0,
        }
    }
}

impl CrashPolicy {
    /// Validate the restart policy before applying it to an extension host.
    pub fn validate(&self) -> Result<()> {
        if self.max_restarts > 0 && self.restart_delay_ms == 0 {
            anyhow::bail!("crash policy restart delay must be greater than zero");
        }
        if !self.backoff_factor.is_finite() {
            anyhow::bail!("crash policy backoff factor must be finite");
        }
        if self.backoff_factor < 1.0 {
            anyhow::bail!("crash policy backoff factor must be at least 1.0");
        }
        Ok(())
    }
}

/// Tracks crash history for a single extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrashRecord {
    /// Extension identifier.
    pub extension_id: String,
    /// Number of times the extension has crashed.
    pub crash_count: u32,
    /// Timestamp of the last crash (milliseconds since epoch).
    pub last_crash: Option<u64>,
    /// Whether the extension has been disabled due to repeated crashes.
    pub disabled: bool,
}

impl CrashRecord {
    /// Create a new crash record for the given extension.
    pub fn new(extension_id: impl Into<String>) -> Self {
        Self {
            extension_id: extension_id.into(),
            crash_count: 0,
            last_crash: None,
            disabled: false,
        }
    }

    /// Create a validated crash record for the given extension.
    pub fn new_checked(extension_id: impl Into<String>) -> Result<Self> {
        let record = Self::new(extension_id);
        record.validate()?;
        Ok(record)
    }

    /// Validate the crash record before using it in extension host decisions.
    pub fn validate(&self) -> Result<()> {
        validate_plugin_id(&self.extension_id, "crash record extension id")?;
        anyhow::ensure!(
            !self.extension_id.contains('/'),
            "crash record extension id cannot contain path separators"
        );
        anyhow::ensure!(
            !self.extension_id.split('.').any(|segment| segment == ".."),
            "crash record extension id cannot contain parent-directory segments"
        );
        Ok(())
    }

    /// Record a new crash occurrence, disabling the extension if it exceeds
    /// the maximum restart count.
    pub fn record_crash(&mut self, policy: &CrashPolicy) {
        self.crash_count += 1;
        self.last_crash = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        if self.crash_count > policy.max_restarts {
            self.disabled = true;
        }
    }

    /// Validate the policy and record before recording a crash.
    pub fn record_crash_checked(&mut self, policy: &CrashPolicy) -> Result<()> {
        self.validate()?;
        policy.validate()?;
        self.record_crash(policy);
        Ok(())
    }

    /// Determine whether the extension should be restarted based on the policy.
    pub fn should_restart(&self, policy: &CrashPolicy) -> bool {
        !self.disabled && self.crash_count <= policy.max_restarts
    }

    /// Validate the policy and record before deciding whether to restart.
    pub fn should_restart_checked(&self, policy: &CrashPolicy) -> Result<bool> {
        self.validate()?;
        policy.validate()?;
        Ok(self.should_restart(policy))
    }

    /// Calculate the delay before the next restart attempt using exponential
    /// backoff.
    pub fn next_restart_delay(&self, policy: &CrashPolicy) -> u64 {
        if self.crash_count == 0 {
            return policy.restart_delay_ms;
        }
        let exponent = (self.crash_count - 1) as f64;
        let delay = policy.restart_delay_ms as f64 * policy.backoff_factor.powf(exponent);
        delay as u64
    }

    /// Validate inputs and calculate the next restart delay.
    ///
    /// Extremely large backoff values saturate to `u64::MAX` so restart
    /// scheduling remains deterministic instead of wrapping.
    pub fn next_restart_delay_checked(&self, policy: &CrashPolicy) -> Result<u64> {
        self.validate()?;
        policy.validate()?;
        if self.crash_count == 0 {
            return Ok(policy.restart_delay_ms);
        }
        let exponent = (self.crash_count - 1) as f64;
        let delay = policy.restart_delay_ms as f64 * policy.backoff_factor.powf(exponent);
        if !delay.is_finite() || delay >= u64::MAX as f64 {
            return Ok(u64::MAX);
        }
        Ok(delay as u64)
    }
}

/// Next action for a checked helper process or plugin host handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperPluginNextAction {
    /// Plugin manifest, permission manifest, IPC schema, and crash policy must be paired first.
    ConfigurePluginContracts,
    /// Permission broker grants and process context must be installed before launch.
    InstallBrokerAndContext,
    /// Restart, heartbeat, or crash policy should be attached to supervision first.
    ConfigureSupervisorPolicy,
    /// Checked helper/plugin launch descriptors can be handed to the native supervisor.
    SpawnNativeHelper,
}

impl HelperPluginNextAction {
    /// Stable key for logs, tests, and generated-agent routing.
    pub fn key(self) -> &'static str {
        match self {
            Self::ConfigurePluginContracts => "configure-plugin-contracts",
            Self::InstallBrokerAndContext => "install-broker-and-context",
            Self::ConfigureSupervisorPolicy => "configure-supervisor-policy",
            Self::SpawnNativeHelper => "spawn-native-helper",
        }
    }
}

/// Checked helper/plugin request inside a launch handoff.
#[derive(Debug, Clone, PartialEq)]
pub enum HelperPluginRequest {
    /// Build and validate a helper launch descriptor.
    LaunchBuilder(HelperProcessLaunchBuilder),
    /// Use an already-built helper launch descriptor.
    Launch(HelperProcessLaunch),
    /// Validate a plugin manifest before loading or spawning its host.
    PluginManifest(PluginManifest),
    /// Validate a plugin permission manifest against granted permission kinds.
    PluginPermissions {
        /// Permission manifest declared by the plugin/package.
        manifest: PluginPermissionManifest,
        /// Permission kinds granted by policy or user approval.
        granted: Vec<PermissionKind>,
    },
    /// Validate an IPC schema used by helper/plugin traffic.
    IpcSchema(IpcSchema),
    /// Validate extension crash/restart policy and current crash record together.
    CrashPolicy {
        /// Crash policy used by the supervisor.
        policy: CrashPolicy,
        /// Optional current crash record for the extension/plugin.
        record: Option<CrashRecord>,
    },
}

impl HelperPluginRequest {
    /// Validate this request without spawning a process or mutating host state.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::LaunchBuilder(builder) => builder.validate(),
            Self::Launch(launch) => launch.validate(),
            Self::PluginManifest(manifest) => manifest.validate(),
            Self::PluginPermissions { manifest, granted } => {
                validate_plugin_permission_manifest(manifest)?;
                let granted = granted.iter().copied().collect();
                manifest.validate(&granted).map_err(|missing| {
                    anyhow::anyhow!(
                        "plugin permission manifest is missing {} required grants",
                        missing.len()
                    )
                })
            }
            Self::IpcSchema(schema) => validate_plugin_ipc_schema(schema),
            Self::CrashPolicy { policy, record } => {
                policy.validate()?;
                if let Some(record) = record {
                    record.validate()?;
                    record.should_restart_checked(policy)?;
                    record.next_restart_delay_checked(policy)?;
                }
                Ok(())
            }
        }
    }

    fn execution_plan(&self) -> Option<HelperProcessExecutionPlan> {
        match self {
            Self::Launch(launch) => Some(launch.execution_plan()),
            Self::LaunchBuilder(builder) => builder
                .clone()
                .build_checked()
                .ok()
                .map(|launch| launch.execution_plan()),
            _ => None,
        }
    }
}

/// Checked handoff for helper-process and plugin-host startup.
#[derive(Debug, Clone, PartialEq)]
pub struct HelperPluginHandoff {
    requests: Vec<HelperPluginRequest>,
    next_action: HelperPluginNextAction,
    launch_count: usize,
    plugin_manifest_count: usize,
    permission_manifest_count: usize,
    ipc_schema_count: usize,
    crash_policy_count: usize,
    helper_plans: Vec<HelperProcessExecutionPlan>,
}

impl HelperPluginHandoff {
    /// Requests included in this handoff.
    pub fn requests(&self) -> &[HelperPluginRequest] {
        &self.requests
    }

    /// Number of checked handoff requests.
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Next action builders or agents should take.
    pub fn next_action(&self) -> HelperPluginNextAction {
        self.next_action
    }

    /// Checked helper execution plans derived from launch descriptors.
    pub fn helper_plans(&self) -> &[HelperProcessExecutionPlan] {
        &self.helper_plans
    }

    /// Number of helper launch descriptors in this handoff.
    pub fn launch_count(&self) -> usize {
        self.launch_count
    }

    /// Number of plugin manifests in this handoff.
    pub fn plugin_manifest_count(&self) -> usize {
        self.plugin_manifest_count
    }

    /// Number of plugin permission manifests in this handoff.
    pub fn permission_manifest_count(&self) -> usize {
        self.permission_manifest_count
    }

    /// Number of IPC schemas in this handoff.
    pub fn ipc_schema_count(&self) -> usize {
        self.ipc_schema_count
    }

    /// Number of crash policies in this handoff.
    pub fn crash_policy_count(&self) -> usize {
        self.crash_policy_count
    }

    /// Whether this handoff includes plugin host contracts.
    pub fn has_plugin_contracts(&self) -> bool {
        self.plugin_manifest_count > 0
            || self.permission_manifest_count > 0
            || self.ipc_schema_count > 0
    }

    /// Whether this handoff includes supervision or crash restart policy.
    pub fn has_supervisor_policy(&self) -> bool {
        self.crash_policy_count > 0
            || self
                .helper_plans
                .iter()
                .any(HelperProcessExecutionPlan::requires_supervisor_policy)
    }

    /// Whether any launch plan still needs broker grants or process context.
    pub fn requires_broker_and_context(&self) -> bool {
        self.helper_plans
            .iter()
            .any(HelperProcessExecutionPlan::requires_broker_and_context)
    }

    /// Whether checked launch descriptors are ready for a native supervisor.
    pub fn can_spawn_native_helpers(&self) -> bool {
        self.next_action == HelperPluginNextAction::SpawnNativeHelper
    }

    /// Content-safe summary for logs, audits, and generated-agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "helper plugin handoff: {} requests, next action {}, launches {}, plugin manifests {}, permission manifests {}, ipc schemas {}, crash policies {}, broker context {}, supervisor {}",
            self.request_count(),
            self.next_action.key(),
            self.launch_count(),
            self.plugin_manifest_count(),
            self.permission_manifest_count(),
            self.ipc_schema_count(),
            self.crash_policy_count(),
            self.requires_broker_and_context(),
            self.has_supervisor_policy()
        )
    }
}

/// Builder for checked helper-process and plugin-host startup handoffs.
#[derive(Debug, Clone, Default)]
pub struct HelperPluginHandoffBuilder {
    requests: Vec<HelperPluginRequest>,
}

impl HelperPluginHandoffBuilder {
    /// Create an empty helper/plugin handoff builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a helper launch builder.
    pub fn launch_builder(mut self, builder: HelperProcessLaunchBuilder) -> Self {
        self.requests
            .push(HelperPluginRequest::LaunchBuilder(builder));
        self
    }

    /// Add an already-built helper launch descriptor.
    pub fn launch(mut self, launch: HelperProcessLaunch) -> Self {
        self.requests.push(HelperPluginRequest::Launch(launch));
        self
    }

    /// Add a plugin manifest.
    pub fn plugin_manifest(mut self, manifest: PluginManifest) -> Self {
        self.requests
            .push(HelperPluginRequest::PluginManifest(manifest));
        self
    }

    /// Add a plugin permission manifest and its granted permission kinds.
    pub fn plugin_permissions(
        mut self,
        manifest: PluginPermissionManifest,
        granted: impl IntoIterator<Item = PermissionKind>,
    ) -> Self {
        self.requests.push(HelperPluginRequest::PluginPermissions {
            manifest,
            granted: granted.into_iter().collect(),
        });
        self
    }

    /// Add an IPC schema.
    pub fn ipc_schema(mut self, schema: IpcSchema) -> Self {
        self.requests.push(HelperPluginRequest::IpcSchema(schema));
        self
    }

    /// Add crash policy without a current crash record.
    pub fn crash_policy(mut self, policy: CrashPolicy) -> Self {
        self.requests.push(HelperPluginRequest::CrashPolicy {
            policy,
            record: None,
        });
        self
    }

    /// Add crash policy with a current crash record.
    pub fn crash_policy_record(mut self, policy: CrashPolicy, record: CrashRecord) -> Self {
        self.requests.push(HelperPluginRequest::CrashPolicy {
            policy,
            record: Some(record),
        });
        self
    }

    /// Validate without consuming this builder.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.requests.is_empty(),
            "helper plugin handoff must include at least one request"
        );
        anyhow::ensure!(
            self.requests.len() <= 32,
            "helper plugin handoff cannot include more than 32 requests"
        );
        for request in &self.requests {
            request.validate()?;
        }
        Ok(())
    }

    /// Build the checked helper/plugin handoff.
    pub fn build_checked(self) -> Result<HelperPluginHandoff> {
        self.validate()?;

        let mut launch_count = 0;
        let mut plugin_manifest_count = 0;
        let mut permission_manifest_count = 0;
        let mut ipc_schema_count = 0;
        let mut crash_policy_count = 0;
        let mut helper_plans = Vec::new();

        for request in &self.requests {
            match request {
                HelperPluginRequest::LaunchBuilder(_) | HelperPluginRequest::Launch(_) => {
                    launch_count += 1;
                    if let Some(plan) = request.execution_plan() {
                        helper_plans.push(plan);
                    }
                }
                HelperPluginRequest::PluginManifest(_) => plugin_manifest_count += 1,
                HelperPluginRequest::PluginPermissions { .. } => permission_manifest_count += 1,
                HelperPluginRequest::IpcSchema(_) => ipc_schema_count += 1,
                HelperPluginRequest::CrashPolicy { .. } => crash_policy_count += 1,
            }
        }

        let next_action = helper_plugin_next_action(
            &helper_plans,
            plugin_manifest_count,
            permission_manifest_count,
            ipc_schema_count,
            crash_policy_count,
        );

        Ok(HelperPluginHandoff {
            requests: self.requests,
            next_action,
            launch_count,
            plugin_manifest_count,
            permission_manifest_count,
            ipc_schema_count,
            crash_policy_count,
            helper_plans,
        })
    }
}

fn helper_plugin_next_action(
    plans: &[HelperProcessExecutionPlan],
    plugin_manifest_count: usize,
    permission_manifest_count: usize,
    ipc_schema_count: usize,
    crash_policy_count: usize,
) -> HelperPluginNextAction {
    if plans
        .iter()
        .any(HelperProcessExecutionPlan::requires_plugin_host_contracts)
        || plugin_manifest_count > 0
        || permission_manifest_count > 0
        || ipc_schema_count > 0
    {
        HelperPluginNextAction::ConfigurePluginContracts
    } else if plans
        .iter()
        .any(HelperProcessExecutionPlan::requires_broker_and_context)
    {
        HelperPluginNextAction::InstallBrokerAndContext
    } else if crash_policy_count > 0
        || plans
            .iter()
            .any(HelperProcessExecutionPlan::requires_supervisor_policy)
    {
        HelperPluginNextAction::ConfigureSupervisorPolicy
    } else {
        HelperPluginNextAction::SpawnNativeHelper
    }
}

fn validate_plugin_permission_manifest(manifest: &PluginPermissionManifest) -> Result<()> {
    validate_plugin_id(&manifest.plugin_id, "plugin permission manifest id")?;
    anyhow::ensure!(
        manifest.required.len() <= 64,
        "plugin permission manifest cannot include more than 64 required permissions"
    );
    anyhow::ensure!(
        manifest.optional.len() <= 64,
        "plugin permission manifest cannot include more than 64 optional permissions"
    );
    let all_permissions = manifest.all_permissions();
    anyhow::ensure!(
        all_permissions.len() == manifest.required.len() + manifest.optional.len(),
        "plugin permission manifest cannot duplicate permissions across required and optional sets"
    );
    Ok(())
}

fn validate_plugin_ipc_schema(schema: &IpcSchema) -> Result<()> {
    anyhow::ensure!(
        schema.version > 0,
        "plugin IPC schema version must be greater than zero"
    );
    anyhow::ensure!(
        schema.min_compatible > 0,
        "plugin IPC schema minimum compatible version must be greater than zero"
    );
    anyhow::ensure!(
        schema.min_compatible <= schema.version,
        "plugin IPC schema minimum compatible version cannot exceed current version"
    );
    anyhow::ensure!(
        schema.message_types.len() <= 128,
        "plugin IPC schema cannot include more than 128 message types"
    );
    let mut seen = HashSet::new();
    for message_type in &schema.message_types {
        validate_plugin_id(message_type, "plugin IPC message type")?;
        anyhow::ensure!(
            seen.insert(message_type),
            "plugin IPC schema message type is duplicated"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plugin API Version Negotiation
// ---------------------------------------------------------------------------

/// The host API version exposed to plugins.
pub const HOST_API_VERSION: &str = "1.0.0";

/// Check whether a plugin's requested API version is compatible with the host.
pub fn is_api_compatible(plugin_api_version: &str) -> bool {
    // For now, only exact major version matching is supported.
    // In the future this can be expanded to semver-compatible ranges.
    plugin_api_version.starts_with("1.")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_validation() {
        let valid = PluginManifest {
            id: "com.example.plugin".to_string(),
            name: "Example".to_string(),
            version: "1.0.0".to_string(),
            api_version: "1.0.0".to_string(),
            description: None,
            author: None,
            entry_point: "plugin.wasm".to_string(),
            execution_model: ExecutionModel::Wasm,
            capabilities: vec![],
            args: vec![],
            contributions: Contributions::default(),
        };
        assert!(valid.validate().is_ok());

        let invalid = PluginManifest {
            id: "".to_string(),
            ..valid.clone()
        };
        assert!(invalid.validate().is_err());
        assert!(valid.high_risk_capabilities().is_empty());
    }

    #[test]
    fn test_manifest_validation_rejects_generated_footguns() {
        let valid = PluginManifest {
            id: "com.example.plugin".to_string(),
            name: "Example".to_string(),
            version: "1.0.0".to_string(),
            api_version: "1.0.0".to_string(),
            description: Some("Example plugin".to_string()),
            author: Some("Example Author".to_string()),
            entry_point: "plugin.wasm".to_string(),
            execution_model: ExecutionModel::Wasm,
            capabilities: vec![],
            args: vec!["--verbose".to_string()],
            contributions: Contributions {
                commands: vec![ContributedCommand {
                    id: "example.say-hello".to_string(),
                    title: "Say Hello".to_string(),
                    keybinding: Some("cmd+shift+h".to_string()),
                }],
                menu_items: vec![ContributedMenuItem {
                    target_menu: "tools".to_string(),
                    label: "Say Hello".to_string(),
                    command_id: "example.say-hello".to_string(),
                }],
                panels: vec![ContributedPanel {
                    id: "example.panel".to_string(),
                    title: "Example Panel".to_string(),
                    default_position: PanelPosition::Right,
                }],
                settings_schema: Some(serde_json::json!({ "type": "object" })),
            },
        };
        assert!(valid.validate().is_ok());

        assert!(
            PluginManifest {
                id: "../outside".to_string(),
                ..valid.clone()
            }
            .validate()
            .is_err()
        );

        assert!(
            PluginManifest {
                id: " com.example.plugin".to_string(),
                ..valid.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            PluginManifest {
                name: "Example\nPlugin".to_string(),
                ..valid.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            PluginManifest {
                version: "1.0.0 beta".to_string(),
                ..valid.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            PluginManifest {
                entry_point: " plugin.wasm".to_string(),
                ..valid.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            PluginManifest {
                args: vec!["--flag\nbad".to_string()],
                ..valid.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            PluginManifest {
                capabilities: vec![Capability::ShellExecute, Capability::ShellExecute],
                ..valid.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            PluginManifest {
                args: vec!["--flag".to_string(); 1_025],
                ..valid
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn plugin_manifest_load_is_bounded_and_rejects_symlinks() {
        let root =
            std::env::temp_dir().join(format!("kael-plugin-manifest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let oversized = root.join("oversized.json");
        std::fs::write(
            &oversized,
            vec![b' '; MAX_PLUGIN_MANIFEST_BYTES as usize + 1],
        )
        .unwrap();
        assert!(PluginManifest::load(&oversized).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = root.join("target.json");
            std::fs::write(&target, b"{}").unwrap();
            let link = root.join("link.json");
            symlink(&target, &link).unwrap();
            assert!(PluginManifest::load(&link).is_err());
        }

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_contribution_validation_rejects_ambiguous_entries() {
        let valid_command = ContributedCommand {
            id: "example.say-hello".to_string(),
            title: "Say Hello".to_string(),
            keybinding: None,
        };

        assert!(
            Contributions {
                commands: vec![valid_command.clone(), valid_command.clone()],
                ..Contributions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Contributions {
                commands: vec![valid_command.clone()],
                menu_items: vec![ContributedMenuItem {
                    target_menu: "tools".to_string(),
                    label: "Missing Command".to_string(),
                    command_id: "example.missing".to_string(),
                }],
                ..Contributions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Contributions {
                panels: vec![
                    ContributedPanel {
                        id: "example.panel".to_string(),
                        title: "Panel".to_string(),
                        default_position: PanelPosition::Right,
                    },
                    ContributedPanel {
                        id: "example.panel".to_string(),
                        title: "Panel 2".to_string(),
                        default_position: PanelPosition::Left,
                    },
                ],
                ..Contributions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Contributions {
                commands: vec![ContributedCommand {
                    id: "bad command".to_string(),
                    title: "Bad".to_string(),
                    keybinding: None,
                }],
                ..Contributions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Contributions {
                settings_schema: Some(serde_json::json!("not an object")),
                ..Contributions::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = PluginManifest {
            id: "com.test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "0.1.0".to_string(),
            api_version: "1.0.0".to_string(),
            description: Some("A test plugin".to_string()),
            author: Some("Test Author".to_string()),
            entry_point: "main.wasm".to_string(),
            execution_model: ExecutionModel::Wasm,
            capabilities: vec![Capability::ClipboardRead],
            args: vec![],
            contributions: Contributions {
                commands: vec![ContributedCommand {
                    id: "test.cmd".to_string(),
                    title: "Test Command".to_string(),
                    keybinding: Some("cmd+t".to_string()),
                }],
                menu_items: vec![],
                panels: vec![],
                settings_schema: None,
            },
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn test_extension_host_lifecycle() {
        let mut host = ExtensionHost::new();
        let manifest = PluginManifest {
            id: "ext-1".to_string(),
            name: "Extension 1".to_string(),
            version: "1.0.0".to_string(),
            api_version: "1.0.0".to_string(),
            description: None,
            author: None,
            entry_point: "ext.wasm".to_string(),
            execution_model: ExecutionModel::Wasm,
            capabilities: vec![],
            args: Vec::new(),
            contributions: Contributions::default(),
        };

        host.load_manifest(manifest.clone()).unwrap();
        assert!(host.get("ext-1").is_some());
        assert!(!host.get("ext-1").unwrap().is_active);

        host.activate("ext-1").unwrap();
        assert!(host.get("ext-1").unwrap().is_active);

        host.deactivate("ext-1").unwrap();
        assert!(!host.get("ext-1").unwrap().is_active);

        host.unload("ext-1").unwrap();
        assert!(host.get("ext-1").is_none());
    }

    #[test]
    fn test_api_version_compatibility() {
        assert!(is_api_compatible("1.0.0"));
        assert!(is_api_compatible("1.2.3"));
        assert!(!is_api_compatible("2.0.0"));
        assert!(!is_api_compatible("0.9.0"));
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = PluginManifest::builder(
            "com.example.builder",
            "Builder",
            "1.0.0",
            "1.0.0",
            "plugin.wasm",
            ExecutionModel::Wasm,
        )
        .description("builder manifest")
        .capability(Capability::ShellExecute)
        .build()
        .unwrap();

        assert_eq!(manifest.description.as_deref(), Some("builder manifest"));
        assert_eq!(
            manifest.high_risk_capabilities(),
            vec![Capability::ShellExecute]
        );
    }

    #[test]
    fn test_extension_host_active_contributions() {
        let mut host = ExtensionHost::new();
        let manifest = PluginManifest::builder(
            "ext-2",
            "Extension 2",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .contributions(Contributions {
            commands: vec![ContributedCommand {
                id: "ext.command".to_string(),
                title: "Extension Command".to_string(),
                keybinding: None,
            }],
            menu_items: vec![ContributedMenuItem {
                target_menu: "file".to_string(),
                label: "Do Thing".to_string(),
                command_id: "ext.command".to_string(),
            }],
            panels: vec![ContributedPanel {
                id: "ext.panel".to_string(),
                title: "Panel".to_string(),
                default_position: PanelPosition::Right,
            }],
            settings_schema: None,
        })
        .build()
        .unwrap();

        host.load_manifest(manifest).unwrap();
        host.activate("ext-2").unwrap();

        assert_eq!(host.active_commands().len(), 1);
        assert_eq!(host.active_menu_items().len(), 1);
        assert_eq!(host.active_panels().len(), 1);
    }

    #[test]
    fn plugin_manifest_and_contribution_summary_is_content_safe() {
        let command = ContributedCommand {
            id: "private.customer.export".to_string(),
            title: "Export Secret Customer List".to_string(),
            keybinding: Some("cmd+shift+e".to_string()),
        };
        let menu_item = ContributedMenuItem {
            target_menu: "private/tools".to_string(),
            label: "Secret Export".to_string(),
            command_id: "private.customer.export".to_string(),
        };
        let panel = ContributedPanel {
            id: "private.customer.panel".to_string(),
            title: "Secret Customer Panel".to_string(),
            default_position: PanelPosition::Floating,
        };
        let contributions = Contributions {
            commands: vec![command.clone()],
            menu_items: vec![menu_item.clone()],
            panels: vec![panel.clone()],
            settings_schema: Some(serde_json::json!({ "type": "object" })),
        };
        let manifest = PluginManifest::builder(
            "com.private.customer-plugin",
            "Secret Customer Plugin",
            "1.2.3",
            "1.0.0",
            "/private/extensions/customer-plugin/main.wasm",
            ExecutionModel::Wasm,
        )
        .description("Handles confidential customer workflows")
        .author("Private Author")
        .arg("--customer-token=secret")
        .capability(Capability::ClipboardRead)
        .contributions(contributions.clone())
        .build()
        .unwrap();

        assert_eq!(ExecutionModel::Wasm.to_text(), "wasm");
        assert_eq!(PanelPosition::Floating.to_text(), "floating");
        assert_eq!(manifest.capability_count(), 1);
        assert_eq!(manifest.arg_count(), 1);
        assert_eq!(contributions.command_count(), 1);
        assert_eq!(contributions.menu_item_count(), 1);
        assert_eq!(contributions.panel_count(), 1);
        assert!(contributions.has_settings_schema());

        let command_summary = command.to_text();
        assert!(command_summary.contains("has_keybinding=true"));
        assert!(!command_summary.contains("private.customer"));
        assert!(!command_summary.contains("Secret Customer"));
        assert!(!command_summary.contains("cmd+shift+e"));

        let menu_summary = menu_item.to_text();
        assert!(!menu_summary.contains("private/tools"));
        assert!(!menu_summary.contains("Secret Export"));
        assert!(!menu_summary.contains("private.customer"));

        let panel_summary = panel.to_text();
        assert!(panel_summary.contains("position=floating"));
        assert!(!panel_summary.contains("Secret Customer"));
        assert!(!panel_summary.contains("private.customer"));

        let manifest_summary = manifest.to_text();
        assert!(manifest_summary.contains("execution_model=wasm"));
        assert!(manifest_summary.contains("capabilities=1"));
        assert!(manifest_summary.contains("args=1"));
        assert!(manifest_summary.contains("has_description=true"));
        assert!(manifest_summary.contains("has_author=true"));
        assert!(manifest_summary.contains("commands=1"));
        assert!(!manifest_summary.contains("Secret Customer"));
        assert!(!manifest_summary.contains("confidential"));
        assert!(!manifest_summary.contains("Private Author"));
        assert!(!manifest_summary.contains("/private/extensions"));
        assert!(!manifest_summary.contains("customer-token"));
    }

    #[test]
    fn extension_host_registry_and_diagnostics_summary_is_content_safe() {
        let mut host = ExtensionHost::new();
        let manifest = PluginManifest::builder(
            "com.private.active",
            "Private Active Extension",
            "1.0.0",
            "1.0.0",
            "private.wasm",
            ExecutionModel::Wasm,
        )
        .command(ContributedCommand {
            id: "private.run".to_string(),
            title: "Run Private Command".to_string(),
            keybinding: None,
        })
        .build()
        .unwrap();

        host.load_manifest_with_options(
            manifest.clone(),
            Some(PathBuf::from("/private/extensions/active")),
            true,
        )
        .unwrap();
        host.activate("com.private.active").unwrap();

        let info_summary = host.get("com.private.active").unwrap().to_text();
        assert!(info_summary.contains("active=true"));
        assert!(info_summary.contains("has_load_path=true"));
        assert!(info_summary.contains("dev_mode=true"));
        assert!(!info_summary.contains("Private Active"));
        assert!(!info_summary.contains("/private/extensions"));
        assert!(!info_summary.contains("private.run"));

        assert_eq!(host.loaded_count(), 1);
        assert_eq!(host.active_count(), 1);
        assert_eq!(host.dev_mode_count(), 1);
        assert_eq!(host.process_count(), 0);
        let host_summary = host.to_text();
        assert_eq!(
            host_summary,
            "extension_host(loaded=1, active=1, dev_mode=1, processes=0, active_commands=1, active_menu_items=0, active_panels=0)"
        );

        let mut registry = ExtensionRegistry::new();
        let extension_manifest = ExtensionManifest {
            id: "com.private.registry".to_string(),
            name: "Private Registry Extension".to_string(),
            version: "2.0.0".to_string(),
            description: "Private registry description".to_string(),
            author: Some("Private Registry Author".to_string()),
            license: Some("Private-License".to_string()),
            contribution_points: vec![
                ContributionPoint::Command {
                    id: "private.registry.command".to_string(),
                    title: "Private Registry Command".to_string(),
                    keybinding: Some("cmd+r".to_string()),
                },
                ContributionPoint::Panel {
                    id: "private.registry.panel".to_string(),
                    title: "Private Registry Panel".to_string(),
                    icon: Some("private-icon".to_string()),
                },
            ],
            permissions: vec!["private.permission".to_string()],
            activation_events: vec!["onPrivateData".to_string()],
        };
        registry.register(extension_manifest.clone()).unwrap();
        registry
            .update_diagnostics(
                "com.private.registry",
                ExtensionState::Error("Secret extension failure".to_string()),
            )
            .unwrap();

        let manifest_summary = extension_manifest.to_text();
        assert!(manifest_summary.contains("contribution_points=2"));
        assert!(manifest_summary.contains("commands=1"));
        assert!(manifest_summary.contains("panels=1"));
        assert!(!manifest_summary.contains("Private Registry"));
        assert!(!manifest_summary.contains("private.registry"));
        assert!(!manifest_summary.contains("Private-License"));
        assert!(!manifest_summary.contains("onPrivateData"));

        let diagnostics = registry.get_diagnostics("com.private.registry").unwrap();
        assert_eq!(diagnostics.state.to_text(), "error");
        assert!(diagnostics.state.has_error_message());
        assert_eq!(
            diagnostics.state.error_message_len_bytes(),
            "Secret extension failure".len()
        );
        assert_eq!(diagnostics.error_count, 1);
        assert!(diagnostics.has_last_error());
        let diagnostics_summary = diagnostics.to_text();
        assert!(diagnostics_summary.contains("state=error"));
        assert!(diagnostics_summary.contains("error_count=1"));
        assert!(!diagnostics_summary.contains("Secret extension failure"));
        assert!(!diagnostics_summary.contains("com.private.registry"));

        assert_eq!(registry.extension_count(), 1);
        assert_eq!(registry.diagnostics_count(), 1);
        assert_eq!(registry.total_error_count(), 1);
        assert_eq!(registry.unhealthy_count(), 1);
        let registry_summary = registry.to_text();
        assert_eq!(
            registry_summary,
            "extension_registry(extensions=1, diagnostics=1, commands=1, panels=1, themes=0, total_errors=1, unhealthy=1)"
        );
    }

    // -----------------------------------------------------------------------
    // Extension Manifest tests
    // -----------------------------------------------------------------------

    fn sample_extension_manifest() -> ExtensionManifest {
        ExtensionManifest {
            id: "com.test.ext".to_string(),
            name: "Test Extension".to_string(),
            version: "1.0.0".to_string(),
            description: "A test extension".to_string(),
            author: Some("Tester".to_string()),
            license: Some("MIT".to_string()),
            contribution_points: vec![],
            permissions: vec!["fs.read".to_string()],
            activation_events: vec!["onStartup".to_string()],
        }
    }

    #[test]
    fn test_extension_manifest_validate_ok() {
        assert!(sample_extension_manifest().validate().is_ok());
    }

    #[test]
    fn test_extension_manifest_validate_empty_id() {
        let mut m = sample_extension_manifest();
        m.id = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_extension_manifest_validate_empty_name() {
        let mut m = sample_extension_manifest();
        m.name = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_extension_manifest_validate_empty_version() {
        let mut m = sample_extension_manifest();
        m.version = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_extension_manifest_validate_empty_description() {
        let mut m = sample_extension_manifest();
        m.description = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_extension_manifest_commands() {
        let m = ExtensionManifest {
            contribution_points: vec![
                ContributionPoint::Command {
                    id: "cmd.one".to_string(),
                    title: "One".to_string(),
                    keybinding: None,
                },
                ContributionPoint::Panel {
                    id: "panel.x".to_string(),
                    title: "X".to_string(),
                    icon: None,
                },
                ContributionPoint::Command {
                    id: "cmd.two".to_string(),
                    title: "Two".to_string(),
                    keybinding: Some("ctrl+t".to_string()),
                },
            ],
            ..sample_extension_manifest()
        };
        let cmds = m.commands();
        assert_eq!(cmds.len(), 2);
        assert!(cmds.contains(&("cmd.one", "One")));
        assert!(cmds.contains(&("cmd.two", "Two")));
    }

    #[test]
    fn test_extension_manifest_panels() {
        let m = ExtensionManifest {
            contribution_points: vec![ContributionPoint::Panel {
                id: "panel.a".to_string(),
                title: "Panel A".to_string(),
                icon: Some("icon.svg".to_string()),
            }],
            ..sample_extension_manifest()
        };
        let panels = m.panels();
        assert_eq!(panels, vec![("panel.a", "Panel A")]);
    }

    #[test]
    fn test_extension_manifest_themes() {
        let m = ExtensionManifest {
            contribution_points: vec![
                ContributionPoint::Theme {
                    id: "dark".to_string(),
                    label: "Dark Theme".to_string(),
                },
                ContributionPoint::Theme {
                    id: "light".to_string(),
                    label: "Light Theme".to_string(),
                },
            ],
            ..sample_extension_manifest()
        };
        let themes = m.themes();
        assert_eq!(themes.len(), 2);
    }

    #[test]
    fn test_extension_manifest_handles_file_extension() {
        let m = ExtensionManifest {
            contribution_points: vec![ContributionPoint::FileType {
                extensions: vec!["rs".to_string(), "toml".to_string()],
                language_id: "rust".to_string(),
            }],
            ..sample_extension_manifest()
        };
        assert!(m.handles_file_extension("rs"));
        assert!(m.handles_file_extension("toml"));
        assert!(!m.handles_file_extension("py"));
    }

    #[test]
    fn test_extension_manifest_serialization() {
        let m = ExtensionManifest {
            contribution_points: vec![ContributionPoint::Setting {
                key: "fontSize".to_string(),
                default_value: serde_json::json!(14),
                description: "Font size".to_string(),
            }],
            ..sample_extension_manifest()
        };
        let json = serde_json::to_string(&m).unwrap();
        let decoded: ExtensionManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, decoded);
    }

    // -----------------------------------------------------------------------
    // ContributionPoint tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_contribution_point_command_variant() {
        let cp = ContributionPoint::Command {
            id: "cmd.test".to_string(),
            title: "Test".to_string(),
            keybinding: Some("ctrl+shift+t".to_string()),
        };
        let json = serde_json::to_string(&cp).unwrap();
        let decoded: ContributionPoint = serde_json::from_str(&json).unwrap();
        assert_eq!(cp, decoded);
    }

    #[test]
    fn test_contribution_point_menu_variant() {
        let cp = ContributionPoint::Menu {
            location: "file".to_string(),
            items: vec![
                PluginMenuItem {
                    command: "save".to_string(),
                    title: "Save".to_string(),
                    group: Some("1_file".to_string()),
                },
                PluginMenuItem {
                    command: "open".to_string(),
                    title: "Open".to_string(),
                    group: None,
                },
            ],
        };
        let json = serde_json::to_string(&cp).unwrap();
        let decoded: ContributionPoint = serde_json::from_str(&json).unwrap();
        assert_eq!(cp, decoded);
    }

    #[test]
    fn test_contribution_point_keybinding_variant() {
        let cp = ContributionPoint::Keybinding {
            command: "editor.format".to_string(),
            key: "ctrl+shift+f".to_string(),
            when: Some("editorFocus".to_string()),
        };
        let json = serde_json::to_string(&cp).unwrap();
        let decoded: ContributionPoint = serde_json::from_str(&json).unwrap();
        assert_eq!(cp, decoded);
    }

    // -----------------------------------------------------------------------
    // ExtensionState & ExtensionDiagnostics tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extension_state_variants() {
        let states = vec![
            ExtensionState::Inactive,
            ExtensionState::Activating,
            ExtensionState::Active,
            ExtensionState::Deactivating,
            ExtensionState::Error("fail".to_string()),
            ExtensionState::Crashed,
        ];
        for state in &states {
            let json = serde_json::to_string(state).unwrap();
            let decoded: ExtensionState = serde_json::from_str(&json).unwrap();
            assert_eq!(*state, decoded);
        }
    }

    #[test]
    fn test_diagnostics_new() {
        let diag = ExtensionDiagnostics::new("ext-1");
        assert_eq!(diag.id, "ext-1");
        assert_eq!(diag.state, ExtensionState::Inactive);
        assert_eq!(diag.error_count, 0);
        assert!(diag.last_error.is_none());
        assert!(diag.activation_time_ms.is_none());
        assert!(diag.memory_usage_bytes.is_none());
    }

    #[test]
    fn test_diagnostics_record_error() {
        let mut diag = ExtensionDiagnostics::new("ext-1");
        diag.record_error("first error");
        assert_eq!(diag.error_count, 1);
        assert_eq!(diag.last_error.as_deref(), Some("first error"));

        diag.record_error("second error");
        assert_eq!(diag.error_count, 2);
        assert_eq!(diag.last_error.as_deref(), Some("second error"));
    }

    // -----------------------------------------------------------------------
    // ExtensionRegistry tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_registry_register_and_get() {
        let mut reg = ExtensionRegistry::new();
        reg.register(sample_extension_manifest()).unwrap();
        assert!(reg.get("com.test.ext").is_some());
        assert_eq!(reg.get("com.test.ext").unwrap().name, "Test Extension");
    }

    #[test]
    fn test_registry_duplicate_registration() {
        let mut reg = ExtensionRegistry::new();
        reg.register(sample_extension_manifest()).unwrap();
        assert!(reg.register(sample_extension_manifest()).is_err());
    }

    #[test]
    fn test_registry_invalid_manifest() {
        let mut reg = ExtensionRegistry::new();
        let mut m = sample_extension_manifest();
        m.id = String::new();
        assert!(reg.register(m).is_err());
    }

    #[test]
    fn test_registry_unregister() {
        let mut reg = ExtensionRegistry::new();
        reg.register(sample_extension_manifest()).unwrap();
        let removed = reg.unregister("com.test.ext").unwrap();
        assert_eq!(removed.id, "com.test.ext");
        assert!(reg.get("com.test.ext").is_none());
    }

    #[test]
    fn test_registry_unregister_missing() {
        let mut reg = ExtensionRegistry::new();
        assert!(reg.unregister("nonexistent").is_err());
    }

    #[test]
    fn test_registry_list() {
        let mut reg = ExtensionRegistry::new();
        let m1 = sample_extension_manifest();
        let mut m2 = sample_extension_manifest();
        m2.id = "com.test.ext2".to_string();
        m2.name = "Ext Two".to_string();
        reg.register(m1).unwrap();
        reg.register(m2).unwrap();
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn test_registry_update_diagnostics() {
        let mut reg = ExtensionRegistry::new();
        reg.register(sample_extension_manifest()).unwrap();

        reg.update_diagnostics("com.test.ext", ExtensionState::Active)
            .unwrap();
        let diag = reg.get_diagnostics("com.test.ext").unwrap();
        assert_eq!(diag.state, ExtensionState::Active);
        assert_eq!(diag.error_count, 0);
    }

    #[test]
    fn test_registry_update_diagnostics_error_state() {
        let mut reg = ExtensionRegistry::new();
        reg.register(sample_extension_manifest()).unwrap();

        reg.update_diagnostics(
            "com.test.ext",
            ExtensionState::Error("something broke".to_string()),
        )
        .unwrap();
        let diag = reg.get_diagnostics("com.test.ext").unwrap();
        assert_eq!(diag.error_count, 1);
        assert_eq!(diag.last_error.as_deref(), Some("something broke"));
    }

    #[test]
    fn test_registry_update_diagnostics_missing() {
        let mut reg = ExtensionRegistry::new();
        assert!(
            reg.update_diagnostics("missing", ExtensionState::Active)
                .is_err()
        );
    }

    #[test]
    fn test_registry_commands() {
        let mut reg = ExtensionRegistry::new();
        let m = ExtensionManifest {
            contribution_points: vec![
                ContributionPoint::Command {
                    id: "cmd.a".to_string(),
                    title: "A".to_string(),
                    keybinding: None,
                },
                ContributionPoint::Command {
                    id: "cmd.b".to_string(),
                    title: "B".to_string(),
                    keybinding: None,
                },
            ],
            ..sample_extension_manifest()
        };
        reg.register(m).unwrap();
        let cmds = reg.commands();
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn test_registry_panels() {
        let mut reg = ExtensionRegistry::new();
        let m = ExtensionManifest {
            contribution_points: vec![ContributionPoint::Panel {
                id: "p.1".to_string(),
                title: "Panel 1".to_string(),
                icon: None,
            }],
            ..sample_extension_manifest()
        };
        reg.register(m).unwrap();
        assert_eq!(reg.panels().len(), 1);
    }

    #[test]
    fn test_registry_themes() {
        let mut reg = ExtensionRegistry::new();
        let m = ExtensionManifest {
            contribution_points: vec![ContributionPoint::Theme {
                id: "monokai".to_string(),
                label: "Monokai".to_string(),
            }],
            ..sample_extension_manifest()
        };
        reg.register(m).unwrap();
        let themes = reg.themes();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0], ("monokai", "Monokai"));
    }

    #[test]
    fn test_registry_file_type_handlers() {
        let mut reg = ExtensionRegistry::new();
        let m = ExtensionManifest {
            contribution_points: vec![ContributionPoint::FileType {
                extensions: vec!["rs".to_string()],
                language_id: "rust".to_string(),
            }],
            ..sample_extension_manifest()
        };
        reg.register(m).unwrap();
        assert_eq!(reg.file_type_handlers("rs").len(), 1);
        assert_eq!(reg.file_type_handlers("py").len(), 0);
    }

    #[test]
    fn test_registry_default() {
        let reg = ExtensionRegistry::default();
        assert!(reg.list().is_empty());
    }

    // -----------------------------------------------------------------------
    // CrashPolicy & CrashRecord tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_crash_policy_default() {
        let policy = CrashPolicy::default();
        assert_eq!(policy.max_restarts, 3);
        assert_eq!(policy.restart_delay_ms, 1000);
        assert!((policy.backoff_factor - 2.0).abs() < f64::EPSILON);
        policy.validate().unwrap();
    }

    #[test]
    fn test_crash_policy_validate_rejects_zero_restart_delay() {
        let policy = CrashPolicy {
            max_restarts: 1,
            restart_delay_ms: 0,
            backoff_factor: 2.0,
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn test_crash_policy_validate_rejects_bad_backoff() {
        let low = CrashPolicy {
            max_restarts: 1,
            restart_delay_ms: 100,
            backoff_factor: 0.5,
        };
        assert!(low.validate().is_err());

        let infinite = CrashPolicy {
            max_restarts: 1,
            restart_delay_ms: 100,
            backoff_factor: f64::INFINITY,
        };
        assert!(infinite.validate().is_err());
    }

    #[test]
    fn test_crash_record_new() {
        let record = CrashRecord::new("ext-1");
        assert_eq!(record.extension_id, "ext-1");
        assert_eq!(record.crash_count, 0);
        assert!(record.last_crash.is_none());
        assert!(!record.disabled);
    }

    #[test]
    fn test_crash_record_new_checked_validates_id() {
        assert!(CrashRecord::new_checked("com.example.ext").is_ok());
        assert!(CrashRecord::new_checked("../bad").is_err());
    }

    #[test]
    fn test_crash_record_single_crash() {
        let policy = CrashPolicy::default();
        let mut record = CrashRecord::new("ext-1");
        record.record_crash(&policy);
        assert_eq!(record.crash_count, 1);
        assert!(record.last_crash.is_some());
        assert!(!record.disabled);
        assert!(record.should_restart(&policy));
    }

    #[test]
    fn test_crash_record_checked_methods_validate_policy_and_id() {
        let bad_policy = CrashPolicy {
            max_restarts: 1,
            restart_delay_ms: 0,
            backoff_factor: 2.0,
        };
        let mut record = CrashRecord::new("com.example.ext");
        assert!(record.record_crash_checked(&bad_policy).is_err());

        let policy = CrashPolicy::default();
        let mut bad_record = CrashRecord::new("bad/id");
        assert!(bad_record.record_crash_checked(&policy).is_err());
        assert!(bad_record.should_restart_checked(&policy).is_err());
        assert!(bad_record.next_restart_delay_checked(&policy).is_err());
    }

    #[test]
    fn test_crash_record_max_restarts() {
        let policy = CrashPolicy {
            max_restarts: 2,
            restart_delay_ms: 100,
            backoff_factor: 1.5,
        };
        let mut record = CrashRecord::new("ext-1");
        record.record_crash(&policy);
        assert!(record.should_restart(&policy));
        record.record_crash(&policy);
        assert!(record.should_restart(&policy));
        record.record_crash(&policy);
        assert!(record.disabled);
        assert!(!record.should_restart(&policy));
    }

    #[test]
    fn test_crash_record_next_restart_delay_zero_crashes() {
        let policy = CrashPolicy::default();
        let record = CrashRecord::new("ext-1");
        assert_eq!(record.next_restart_delay(&policy), 1000);
    }

    #[test]
    fn test_crash_record_next_restart_delay_backoff() {
        let policy = CrashPolicy {
            max_restarts: 5,
            restart_delay_ms: 1000,
            backoff_factor: 2.0,
        };
        let mut record = CrashRecord::new("ext-1");
        record.crash_count = 1;
        assert_eq!(record.next_restart_delay(&policy), 1000);
        record.crash_count = 2;
        assert_eq!(record.next_restart_delay(&policy), 2000);
        record.crash_count = 3;
        assert_eq!(record.next_restart_delay(&policy), 4000);
    }

    #[test]
    fn test_crash_record_next_restart_delay_checked_saturates() {
        let policy = CrashPolicy {
            max_restarts: u32::MAX,
            restart_delay_ms: 1000,
            backoff_factor: 10.0,
        };
        let mut record = CrashRecord::new_checked("com.example.ext").unwrap();
        record.crash_count = u32::MAX;
        assert_eq!(
            record.next_restart_delay_checked(&policy).unwrap(),
            u64::MAX
        );
    }

    #[test]
    fn test_crash_record_serialization() {
        let policy = CrashPolicy::default();
        let mut record = CrashRecord::new("ext-1");
        record.record_crash(&policy);

        let json = serde_json::to_string(&record).unwrap();
        let decoded: CrashRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn test_crash_policy_serialization() {
        let policy = CrashPolicy {
            max_restarts: 5,
            restart_delay_ms: 500,
            backoff_factor: 1.5,
        };
        let json = serde_json::to_string(&policy).unwrap();
        let decoded: CrashPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, decoded);
    }

    #[test]
    fn helper_plugin_handoff_validates_plugin_host_startup() {
        let executable = std::env::current_exe().unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.example.notes",
            "Notes Plugin",
            "1.0.0",
            "1.0.0",
            "notes-plugin",
            ExecutionModel::ExternalProcess,
        )
        .capability(Capability::Network {
            hosts: vec!["api.example.com".into()],
        })
        .build()
        .unwrap();
        let permission_manifest = PluginPermissionManifest {
            plugin_id: "com.example.notes".into(),
            required: vec![PermissionKind::Network],
            optional: vec![PermissionKind::Notifications],
        };
        let schema = IpcSchema::new(2, 1, vec!["plugin.ping".into(), "plugin.progress".into()]);

        let handoff = HelperPluginHandoffBuilder::new()
            .launch_builder(HelperProcessLaunch::plugin_host(
                crate::ProcessId(90),
                "notes-plugin-host",
                &executable,
            ))
            .plugin_manifest(manifest)
            .plugin_permissions(permission_manifest, [PermissionKind::Network])
            .ipc_schema(schema)
            .crash_policy_record(
                CrashPolicy::default(),
                CrashRecord::new_checked("com.example.notes").unwrap(),
            )
            .build_checked()
            .unwrap();

        assert_eq!(handoff.request_count(), 5);
        assert_eq!(handoff.launch_count(), 1);
        assert_eq!(handoff.plugin_manifest_count(), 1);
        assert_eq!(handoff.permission_manifest_count(), 1);
        assert_eq!(handoff.ipc_schema_count(), 1);
        assert_eq!(handoff.crash_policy_count(), 1);
        assert!(handoff.has_plugin_contracts());
        assert!(handoff.has_supervisor_policy());
        assert!(handoff.requires_broker_and_context());
        assert!(!handoff.can_spawn_native_helpers());
        assert_eq!(
            handoff.next_action(),
            HelperPluginNextAction::ConfigurePluginContracts
        );
        assert_eq!(
            HelperPluginNextAction::SpawnNativeHelper.key(),
            "spawn-native-helper"
        );
        assert_eq!(
            handoff.to_text(),
            "helper plugin handoff: 5 requests, next action configure-plugin-contracts, launches 1, plugin manifests 1, permission manifests 1, ipc schemas 1, crash policies 1, broker context true, supervisor true"
        );

        let utility = HelperPluginHandoffBuilder::new()
            .launch_builder(HelperProcessLaunch::utility(
                crate::ProcessId(91),
                "thumbnailer",
                &executable,
            ))
            .build_checked()
            .unwrap();
        assert_eq!(
            utility.next_action(),
            HelperPluginNextAction::SpawnNativeHelper
        );
        assert!(!utility.has_supervisor_policy());
        assert!(utility.can_spawn_native_helpers());
    }

    #[test]
    fn helper_plugin_handoff_rejects_invalid_generated_contracts() {
        let executable = std::env::current_exe().unwrap();
        let permission_manifest = PluginPermissionManifest {
            plugin_id: "com.example.notes".into(),
            required: vec![PermissionKind::Network],
            optional: Vec::new(),
        };

        assert!(
            HelperPluginHandoffBuilder::new()
                .plugin_permissions(permission_manifest, [])
                .build_checked()
                .is_err()
        );
        assert!(
            HelperPluginHandoffBuilder::new()
                .ipc_schema(IpcSchema::new(
                    1,
                    1,
                    vec!["plugin.ping".into(), "plugin.ping".into()],
                ))
                .build_checked()
                .is_err()
        );
        assert!(
            HelperPluginHandoffBuilder::new()
                .crash_policy(CrashPolicy {
                    max_restarts: 2,
                    restart_delay_ms: 0,
                    backoff_factor: 2.0,
                })
                .build_checked()
                .is_err()
        );
        assert!(
            HelperPluginHandoffBuilder::new()
                .launch_builder(HelperProcessLaunch::utility(
                    crate::ProcessId(92),
                    "bad\0helper",
                    &executable,
                ))
                .build_checked()
                .is_err()
        );
        assert!(HelperPluginHandoffBuilder::new().build_checked().is_err());
    }
}
