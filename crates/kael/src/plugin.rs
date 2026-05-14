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
}
