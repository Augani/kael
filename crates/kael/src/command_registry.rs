use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// A unique identifier for a palette command.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaletteCommandId(String);

impl PaletteCommandId {
    /// Creates a new [`PaletteCommandId`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<String>> From<T> for PaletteCommandId {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Display for PaletteCommandId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata describing a command for the command palette, including its label,
/// category, and optional keybinding hint and icon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptor {
    /// The unique identifier for this command.
    pub id: PaletteCommandId,
    /// A human-readable label displayed in menus and the command palette.
    pub label: String,
    /// The category this command belongs to (e.g. "File", "Edit", "View").
    pub category: String,
    /// An optional hint string describing the keybinding (e.g. "Cmd+S").
    pub keybinding: Option<String>,
    /// An optional icon identifier for UI display.
    pub icon: Option<String>,
}

/// A searchable palette of command descriptors for command palette UX.
///
/// Commands are indexed by their [`PaletteCommandId`] and can be searched by
/// label or filtered by category. This complements the existing
/// `CommandRegistry` in `app_runtime` which handles command execution.
#[derive(Debug)]
pub struct CommandPalette {
    commands: HashMap<PaletteCommandId, CommandDescriptor>,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }
}

impl CommandPalette {
    /// Creates a new empty [`CommandPalette`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a command descriptor. Returns an error if a command with the
    /// same id is already registered.
    pub fn register(&mut self, descriptor: CommandDescriptor) -> Result<()> {
        if self.commands.contains_key(&descriptor.id) {
            return Err(anyhow!("command '{}' is already registered", descriptor.id));
        }
        self.commands.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    /// Removes a command by its id. Returns an error if the command is not found.
    pub fn unregister(&mut self, id: &PaletteCommandId) -> Result<CommandDescriptor> {
        self.commands
            .remove(id)
            .ok_or_else(|| anyhow!("command '{}' is not registered", id))
    }

    /// Returns a reference to the descriptor for the given command id.
    pub fn get(&self, id: &PaletteCommandId) -> Option<&CommandDescriptor> {
        self.commands.get(id)
    }

    /// Searches for commands whose label or category contains the query string
    /// (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&CommandDescriptor> {
        let query_lower = query.to_lowercase();
        self.commands
            .values()
            .filter(|descriptor| {
                descriptor.label.to_lowercase().contains(&query_lower)
                    || descriptor.category.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Returns references to all registered command descriptors.
    pub fn commands(&self) -> Vec<&CommandDescriptor> {
        self.commands.values().collect()
    }

    /// Returns references to all command descriptors in the given category.
    pub fn commands_in_category(&self, category: &str) -> Vec<&CommandDescriptor> {
        let category_lower = category.to_lowercase();
        self.commands
            .values()
            .filter(|descriptor| descriptor.category.to_lowercase() == category_lower)
            .collect()
    }

    /// Returns the total number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns whether the palette is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_descriptor(id: &str, label: &str, category: &str) -> CommandDescriptor {
        CommandDescriptor {
            id: PaletteCommandId::new(id),
            label: label.to_string(),
            category: category.to_string(),
            keybinding: None,
            icon: None,
        }
    }

    #[test]
    fn register_and_get() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("file.save", "Save File", "File"))
            .unwrap();
        let result = palette.get(&PaletteCommandId::new("file.save"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().label, "Save File");
    }

    #[test]
    fn register_duplicate_returns_error() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("file.save", "Save File", "File"))
            .unwrap();
        assert!(palette
            .register(make_descriptor("file.save", "Save Again", "File"))
            .is_err());
    }

    #[test]
    fn unregister_removes_command() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("file.save", "Save File", "File"))
            .unwrap();
        let removed = palette
            .unregister(&PaletteCommandId::new("file.save"))
            .unwrap();
        assert_eq!(removed.label, "Save File");
        assert!(palette.get(&PaletteCommandId::new("file.save")).is_none());
    }

    #[test]
    fn search_matches_label_case_insensitive() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("file.save", "Save File", "File"))
            .unwrap();
        palette
            .register(make_descriptor("edit.undo", "Undo", "Edit"))
            .unwrap();
        let results = palette.search("SAVE");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_matches_category() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("file.save", "Save", "File"))
            .unwrap();
        palette
            .register(make_descriptor("edit.undo", "Undo", "Edit"))
            .unwrap();
        let results = palette.search("edit");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_empty_returns_all() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("a", "Alpha", "Cat1"))
            .unwrap();
        palette
            .register(make_descriptor("b", "Beta", "Cat2"))
            .unwrap();
        assert_eq!(palette.search("").len(), 2);
    }

    #[test]
    fn commands_in_category_filters() {
        let mut palette = CommandPalette::new();
        palette
            .register(make_descriptor("file.save", "Save", "File"))
            .unwrap();
        palette
            .register(make_descriptor("file.open", "Open", "File"))
            .unwrap();
        palette
            .register(make_descriptor("edit.undo", "Undo", "Edit"))
            .unwrap();
        assert_eq!(palette.commands_in_category("File").len(), 2);
        assert_eq!(palette.commands_in_category("Edit").len(), 1);
        assert!(palette.commands_in_category("View").is_empty());
    }

    #[test]
    fn len_and_is_empty() {
        let mut palette = CommandPalette::new();
        assert!(palette.is_empty());
        assert_eq!(palette.len(), 0);
        palette.register(make_descriptor("a", "A", "C")).unwrap();
        assert!(!palette.is_empty());
        assert_eq!(palette.len(), 1);
    }

    #[test]
    fn descriptor_with_keybinding_and_icon() {
        let mut palette = CommandPalette::new();
        let descriptor = CommandDescriptor {
            id: PaletteCommandId::new("file.save"),
            label: "Save File".to_string(),
            category: "File".to_string(),
            keybinding: Some("Cmd+S".to_string()),
            icon: Some("save-icon".to_string()),
        };
        palette.register(descriptor).unwrap();
        let result = palette.get(&PaletteCommandId::new("file.save")).unwrap();
        assert_eq!(result.keybinding.as_deref(), Some("Cmd+S"));
        assert_eq!(result.icon.as_deref(), Some("save-icon"));
    }

    #[test]
    fn register_after_unregister() {
        let mut palette = CommandPalette::new();
        palette.register(make_descriptor("a", "V1", "C")).unwrap();
        palette.unregister(&PaletteCommandId::new("a")).unwrap();
        palette.register(make_descriptor("a", "V2", "C")).unwrap();
        assert_eq!(
            palette.get(&PaletteCommandId::new("a")).unwrap().label,
            "V2"
        );
    }

    #[test]
    fn default_palette_is_empty() {
        let palette = CommandPalette::default();
        assert!(palette.is_empty());
    }

    #[test]
    fn palette_command_id_display() {
        let id = PaletteCommandId::new("file.save");
        assert_eq!(format!("{}", id), "file.save");
    }
}
