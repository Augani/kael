//! Plugin and extension architecture for GPUI.
//!
//! This module defines the plugin manifest format, capability model for
//! extensions, and the extension host contract. Plugins run in isolated
//! processes (building on the process-isolation model) and communicate with
//! the main application via typed IPC.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::security::Capability;

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

impl PluginManifest {
    /// Validate the manifest for well-formedness.
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            anyhow::bail!("plugin manifest: id must not be empty");
        }
        if self.name.is_empty() {
            anyhow::bail!("plugin manifest: name must not be empty");
        }
        if self.version.is_empty() {
            anyhow::bail!("plugin manifest: version must not be empty");
        }
        if self.entry_point.is_empty() {
            anyhow::bail!("plugin manifest: entry_point must not be empty");
        }
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
        let contents = std::fs::read_to_string(path)?;
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

    /// Determine whether the extension should be restarted based on the policy.
    pub fn should_restart(&self, policy: &CrashPolicy) -> bool {
        !self.disabled && self.crash_count <= policy.max_restarts
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
}
