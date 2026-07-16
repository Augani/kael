use std::collections::HashMap;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

const MAX_PALETTE_COMMANDS: usize = 4_096;
const MAX_PALETTE_QUERY_BYTES: usize = 512;

/// A unique identifier for a palette command.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaletteCommandId(String);

impl PaletteCommandId {
    /// Creates a new [`PaletteCommandId`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Creates a command id after validating its generated identifier shape.
    pub fn new_checked(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        validate_command_id(&id, "palette command id")?;
        Ok(Self(id))
    }

    /// Returns the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Byte length of the command id without exposing the id text.
    pub fn len_bytes(&self) -> usize {
        self.0.len()
    }

    /// Returns true when the command id is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Content-safe command id summary.
    pub fn to_text(&self) -> String {
        format!(
            "palette_command_id(len_bytes={}, empty={})",
            self.len_bytes(),
            self.is_empty()
        )
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

impl CommandDescriptor {
    /// Validate the descriptor before publishing it to a command palette.
    pub fn validate(&self) -> Result<()> {
        validate_command_descriptor(self)
    }
    /// Byte length of the command id without exposing it.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len_bytes()
    }

    /// Byte length of the command label without exposing it.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Byte length of the command category without exposing it.
    pub fn category_len_bytes(&self) -> usize {
        self.category.len()
    }

    /// Returns true when a keybinding hint is configured.
    pub fn has_keybinding(&self) -> bool {
        self.keybinding.is_some()
    }

    /// Byte length of the keybinding hint without exposing it.
    pub fn keybinding_len_bytes(&self) -> usize {
        self.keybinding
            .as_ref()
            .map_or(0, |keybinding| keybinding.len())
    }

    /// Returns true when an icon identifier is configured.
    pub fn has_icon(&self) -> bool {
        self.icon.is_some()
    }

    /// Byte length of the icon identifier without exposing it.
    pub fn icon_len_bytes(&self) -> usize {
        self.icon.as_ref().map_or(0, |icon| icon.len())
    }

    /// Content-safe command descriptor summary.
    pub fn to_text(&self) -> String {
        format!(
            "command_descriptor(id_len_bytes={}, label_len_bytes={}, category_len_bytes={}, has_keybinding={}, keybinding_len_bytes={}, has_icon={}, icon_len_bytes={})",
            self.id_len_bytes(),
            self.label_len_bytes(),
            self.category_len_bytes(),
            self.has_keybinding(),
            self.keybinding_len_bytes(),
            self.has_icon(),
            self.icon_len_bytes()
        )
    }
}

/// A searchable palette of command descriptors for command palette UX.
///
/// Commands are indexed by their [`PaletteCommandId`] and can be searched by
/// label or filtered by category. This complements the existing
/// `CommandRegistry` in `app_runtime` which handles command execution.
#[derive(Debug, Default)]
pub struct CommandPalette {
    commands: HashMap<PaletteCommandId, CommandDescriptor>,
}

impl CommandPalette {
    /// Creates a new empty [`CommandPalette`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a command descriptor. Returns an error if a command with the
    /// same id is already registered.
    pub fn register(&mut self, descriptor: CommandDescriptor) -> Result<()> {
        descriptor.validate()?;
        if self.commands.contains_key(&descriptor.id) {
            return Err(anyhow!("command id is already registered"));
        }
        anyhow::ensure!(
            self.commands.len() < MAX_PALETTE_COMMANDS,
            "command palette cannot contain more than {MAX_PALETTE_COMMANDS} commands"
        );
        self.commands.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    /// Removes a command by its id. Returns an error if the command is not found.
    pub fn unregister(&mut self, id: &PaletteCommandId) -> Result<CommandDescriptor> {
        validate_command_id(id.as_str(), "palette command id")?;
        self.commands
            .remove(id)
            .ok_or_else(|| anyhow!("command id is not registered"))
    }

    /// Returns a reference to the descriptor for the given command id.
    pub fn get(&self, id: &PaletteCommandId) -> Option<&CommandDescriptor> {
        self.commands.get(id)
    }

    /// Searches for commands whose label or category contains the query string
    /// (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&CommandDescriptor> {
        self.search_checked(query).unwrap_or_default()
    }

    /// Searches for commands after validating a generated palette query.
    pub fn search_checked(&self, query: &str) -> Result<Vec<&CommandDescriptor>> {
        validate_palette_query(query)?;
        let query_lower = query.to_lowercase();
        let mut commands =
            self.commands
                .values()
                .filter(|descriptor| {
                    descriptor.label.to_lowercase().contains(&query_lower)
                        || descriptor.category.to_lowercase().contains(&query_lower)
                        || descriptor.id.as_str().to_lowercase().contains(&query_lower)
                        || descriptor.keybinding.as_ref().is_some_and(|keybinding| {
                            keybinding.to_lowercase().contains(&query_lower)
                        })
                })
                .collect::<Vec<_>>();
        sort_palette_commands(&mut commands);
        Ok(commands)
    }

    /// Returns references to all registered command descriptors.
    pub fn commands(&self) -> Vec<&CommandDescriptor> {
        let mut commands = self.commands.values().collect::<Vec<_>>();
        commands.sort_by_cached_key(|descriptor| {
            (
                descriptor.category.to_lowercase(),
                descriptor.label.to_lowercase(),
                descriptor.id.as_str().to_string(),
            )
        });
        commands
    }

    /// Returns references to all command descriptors in the given category.
    pub fn commands_in_category(&self, category: &str) -> Vec<&CommandDescriptor> {
        if category.len() > 128 || category.chars().any(char::is_control) {
            return Vec::new();
        }
        let category_lower = category.to_lowercase();
        let mut commands = self
            .commands
            .values()
            .filter(|descriptor| descriptor.category.to_lowercase() == category_lower)
            .collect::<Vec<_>>();
        sort_palette_commands(&mut commands);
        commands
    }

    /// Returns the total number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns whether the palette is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Count commands with keybinding hints.
    pub fn keybinding_count(&self) -> usize {
        self.commands
            .values()
            .filter(|descriptor| descriptor.has_keybinding())
            .count()
    }

    /// Count commands with icons.
    pub fn icon_count(&self) -> usize {
        self.commands
            .values()
            .filter(|descriptor| descriptor.has_icon())
            .count()
    }

    /// Count distinct command categories.
    pub fn category_count(&self) -> usize {
        self.commands
            .values()
            .map(|descriptor| descriptor.category.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// Content-safe command palette summary.
    pub fn to_text(&self) -> String {
        format!(
            "command_palette(commands={}, categories={}, keybindings={}, icons={}, empty={})",
            self.len(),
            self.category_count(),
            self.keybinding_count(),
            self.icon_count(),
            self.is_empty()
        )
    }
}

fn sort_palette_commands(commands: &mut Vec<&CommandDescriptor>) {
    commands.sort_by_cached_key(|descriptor| {
        (
            descriptor.label.to_lowercase(),
            descriptor.id.as_str().to_string(),
        )
    });
}

fn validate_palette_query(query: &str) -> Result<()> {
    anyhow::ensure!(
        query.len() <= MAX_PALETTE_QUERY_BYTES,
        "command palette query cannot exceed {MAX_PALETTE_QUERY_BYTES} bytes"
    );
    anyhow::ensure!(
        !query.chars().any(char::is_control),
        "command palette query cannot contain control characters"
    );
    Ok(())
}

/// Next action for a checked command and IPC handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandIpcNextAction {
    /// Register an app-owned command.
    RegisterCommand,
    /// Publish a command palette descriptor.
    PublishPaletteDescriptor,
    /// Send a typed IPC request.
    SendIpcRequest,
    /// Send a typed IPC response.
    SendIpcResponse,
    /// Send typed IPC progress.
    SendIpcProgress,
    /// Send typed IPC cancellation.
    SendIpcCancel,
    /// Route extension RPC traffic.
    RouteExtensionRpc,
    /// Use a hosted WebView bridge for an explicit browser island.
    UseHostedBridge,
}

impl CommandIpcNextAction {
    /// Stable action key for generated routing.
    pub fn key(self) -> &'static str {
        match self {
            Self::RegisterCommand => "register-command",
            Self::PublishPaletteDescriptor => "publish-palette-descriptor",
            Self::SendIpcRequest => "send-ipc-request",
            Self::SendIpcResponse => "send-ipc-response",
            Self::SendIpcProgress => "send-ipc-progress",
            Self::SendIpcCancel => "send-ipc-cancel",
            Self::RouteExtensionRpc => "route-extension-rpc",
            Self::UseHostedBridge => "use-hosted-bridge",
        }
    }
}

/// Checked command or IPC request for generated app actions and helpers.
#[derive(Debug, Clone)]
pub enum CommandIpcRequest {
    /// App command registration metadata.
    RegisterCommand {
        /// Command identifier.
        id: String,
        /// Command display name.
        name: String,
    },
    /// Command palette descriptor.
    PaletteDescriptor(CommandDescriptor),
    /// Typed IPC request envelope.
    IpcRequest {
        /// Correlation identifier for the request.
        correlation_id: u64,
    },
    /// Typed IPC response envelope.
    IpcResponse {
        /// Correlation identifier for the response.
        correlation_id: u64,
        /// Whether the response is successful.
        success: bool,
    },
    /// Typed IPC progress envelope.
    IpcProgress {
        /// Correlation identifier for the progress update.
        correlation_id: u64,
    },
    /// Typed IPC cancellation envelope.
    IpcCancel {
        /// Correlation identifier for the cancellation.
        correlation_id: u64,
    },
    /// Extension RPC message family.
    ExtensionRpc {
        /// Redacted message family label.
        message_kind: String,
    },
    /// Hosted page bridge message family.
    HostedBridge {
        /// Redacted hosted bridge message family label.
        message_kind: String,
    },
}

impl CommandIpcRequest {
    /// Whether this request registers an app command.
    pub fn is_register_command(&self) -> bool {
        matches!(self, Self::RegisterCommand { .. })
    }

    /// Whether this request publishes a palette descriptor.
    pub fn is_palette_descriptor(&self) -> bool {
        matches!(self, Self::PaletteDescriptor(_))
    }

    /// Whether this request sends a typed IPC request.
    pub fn is_ipc_request(&self) -> bool {
        matches!(self, Self::IpcRequest { .. })
    }

    /// Whether this request sends a typed IPC response.
    pub fn is_ipc_response(&self) -> bool {
        matches!(self, Self::IpcResponse { .. })
    }

    /// Whether this request sends typed IPC progress.
    pub fn is_ipc_progress(&self) -> bool {
        matches!(self, Self::IpcProgress { .. })
    }

    /// Whether this request sends typed IPC cancellation.
    pub fn is_ipc_cancel(&self) -> bool {
        matches!(self, Self::IpcCancel { .. })
    }

    /// Whether this request routes extension RPC traffic.
    pub fn is_extension_rpc(&self) -> bool {
        matches!(self, Self::ExtensionRpc { .. })
    }

    /// Whether this request uses a hosted bridge.
    pub fn is_hosted_bridge(&self) -> bool {
        matches!(self, Self::HostedBridge { .. })
    }

    /// Next action implied by this request.
    pub fn next_action(&self) -> CommandIpcNextAction {
        match self {
            Self::RegisterCommand { .. } => CommandIpcNextAction::RegisterCommand,
            Self::PaletteDescriptor(_) => CommandIpcNextAction::PublishPaletteDescriptor,
            Self::IpcRequest { .. } => CommandIpcNextAction::SendIpcRequest,
            Self::IpcResponse { .. } => CommandIpcNextAction::SendIpcResponse,
            Self::IpcProgress { .. } => CommandIpcNextAction::SendIpcProgress,
            Self::IpcCancel { .. } => CommandIpcNextAction::SendIpcCancel,
            Self::ExtensionRpc { .. } => CommandIpcNextAction::RouteExtensionRpc,
            Self::HostedBridge { .. } => CommandIpcNextAction::UseHostedBridge,
        }
    }

    /// Command identifier when present.
    pub fn command_id(&self) -> Option<&str> {
        match self {
            Self::RegisterCommand { id, .. } => Some(id),
            Self::PaletteDescriptor(descriptor) => Some(descriptor.id.as_str()),
            _ => None,
        }
    }

    /// Command name when present.
    pub fn command_name(&self) -> Option<&str> {
        match self {
            Self::RegisterCommand { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Palette descriptor when present.
    pub fn palette_descriptor(&self) -> Option<&CommandDescriptor> {
        match self {
            Self::PaletteDescriptor(descriptor) => Some(descriptor),
            _ => None,
        }
    }

    /// Whether this request has a correlation id.
    pub fn has_correlation_id(&self) -> bool {
        matches!(
            self,
            Self::IpcRequest { .. }
                | Self::IpcResponse { .. }
                | Self::IpcProgress { .. }
                | Self::IpcCancel { .. }
        )
    }

    /// Whether this request has a message family label.
    pub fn has_message_kind(&self) -> bool {
        matches!(
            self,
            Self::ExtensionRpc { message_kind } | Self::HostedBridge { message_kind }
                if !message_kind.is_empty()
        )
    }

    /// Content-safe request summary.
    pub fn to_text(&self) -> String {
        let detail = match self {
            Self::RegisterCommand { id, name } => format!(
                "command registration: id_len_bytes {}, name_len_bytes {}",
                id.len(),
                name.len()
            ),
            Self::PaletteDescriptor(descriptor) => descriptor.to_text(),
            Self::IpcRequest { .. } => "typed ipc: request".to_string(),
            Self::IpcResponse { success, .. } => {
                format!("typed ipc: response success {}", success)
            }
            Self::IpcProgress { .. } => "typed ipc: progress".to_string(),
            Self::IpcCancel { .. } => "typed ipc: cancel".to_string(),
            Self::ExtensionRpc { .. } => {
                format!("extension rpc: message kind {}", self.has_message_kind())
            }
            Self::HostedBridge { .. } => {
                format!("hosted bridge: message kind {}", self.has_message_kind())
            }
        };
        format!(
            "command ipc request: action {}, correlation {}, {}",
            self.next_action().key(),
            self.has_correlation_id(),
            detail
        )
    }

    /// Validate the command or IPC request before routing.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::RegisterCommand { id, name } => {
                validate_command_id(id, "command id")?;
                validate_command_text(name, "command name", 128)?;
                Ok(())
            }
            Self::PaletteDescriptor(descriptor) => validate_command_descriptor(descriptor),
            Self::IpcRequest { correlation_id }
            | Self::IpcProgress { correlation_id }
            | Self::IpcCancel { correlation_id } => validate_correlation_id(*correlation_id),
            Self::IpcResponse { correlation_id, .. } => validate_correlation_id(*correlation_id),
            Self::ExtensionRpc { message_kind } => {
                validate_command_text(message_kind, "extension rpc message kind", 64)
            }
            Self::HostedBridge { message_kind } => {
                validate_command_text(message_kind, "hosted bridge message kind", 64)
            }
        }
    }
}

/// Builder for checked command and IPC routing handoffs.
#[derive(Debug, Clone)]
pub struct CommandIpcHandoffBuilder {
    request: CommandIpcRequest,
}

impl CommandIpcHandoffBuilder {
    /// Handoff for app command registration.
    pub fn register_command(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            request: CommandIpcRequest::RegisterCommand {
                id: id.into(),
                name: name.into(),
            },
        }
    }

    /// Handoff for publishing a palette descriptor.
    pub fn palette_descriptor(descriptor: CommandDescriptor) -> Self {
        Self {
            request: CommandIpcRequest::PaletteDescriptor(descriptor),
        }
    }

    /// Handoff for sending a typed IPC request.
    pub fn ipc_request(correlation_id: u64) -> Self {
        Self {
            request: CommandIpcRequest::IpcRequest { correlation_id },
        }
    }

    /// Handoff for sending a typed IPC response.
    pub fn ipc_response(correlation_id: u64, success: bool) -> Self {
        Self {
            request: CommandIpcRequest::IpcResponse {
                correlation_id,
                success,
            },
        }
    }

    /// Handoff for sending typed IPC progress.
    pub fn ipc_progress(correlation_id: u64) -> Self {
        Self {
            request: CommandIpcRequest::IpcProgress { correlation_id },
        }
    }

    /// Handoff for sending typed IPC cancellation.
    pub fn ipc_cancel(correlation_id: u64) -> Self {
        Self {
            request: CommandIpcRequest::IpcCancel { correlation_id },
        }
    }

    /// Handoff for extension RPC traffic.
    pub fn extension_rpc(message_kind: impl Into<String>) -> Self {
        Self {
            request: CommandIpcRequest::ExtensionRpc {
                message_kind: message_kind.into(),
            },
        }
    }

    /// Handoff for hosted WebView bridge traffic.
    pub fn hosted_bridge(message_kind: impl Into<String>) -> Self {
        Self {
            request: CommandIpcRequest::HostedBridge {
                message_kind: message_kind.into(),
            },
        }
    }

    /// Request carried by this builder.
    pub fn request(&self) -> &CommandIpcRequest {
        &self.request
    }

    /// Next action implied by this builder.
    pub fn next_action(&self) -> CommandIpcNextAction {
        self.request.next_action()
    }

    /// Content-safe builder summary.
    pub fn to_text(&self) -> String {
        format!("command ipc handoff builder: {}", self.request.to_text())
    }

    /// Validate the handoff before routing.
    pub fn validate(&self) -> Result<()> {
        self.request.validate()
    }

    /// Build a checked handoff.
    pub fn build_checked(self) -> Result<CommandIpcHandoff> {
        self.validate()?;
        let next_action = self.request.next_action();
        Ok(CommandIpcHandoff {
            request: self.request,
            next_action,
        })
    }
}

/// Checked handoff for command registration, palettes, IPC, and bridge routing.
#[derive(Debug, Clone)]
pub struct CommandIpcHandoff {
    request: CommandIpcRequest,
    next_action: CommandIpcNextAction,
}

impl CommandIpcHandoff {
    /// Build a checked app command registration handoff.
    pub fn register_command(id: impl Into<String>, name: impl Into<String>) -> Result<Self> {
        CommandIpcHandoffBuilder::register_command(id, name).build_checked()
    }

    /// Build a checked palette descriptor handoff.
    pub fn palette_descriptor(descriptor: CommandDescriptor) -> Result<Self> {
        CommandIpcHandoffBuilder::palette_descriptor(descriptor).build_checked()
    }

    /// Build a checked typed IPC request handoff.
    pub fn ipc_request(correlation_id: u64) -> Result<Self> {
        CommandIpcHandoffBuilder::ipc_request(correlation_id).build_checked()
    }

    /// Build a checked typed IPC response handoff.
    pub fn ipc_response(correlation_id: u64, success: bool) -> Result<Self> {
        CommandIpcHandoffBuilder::ipc_response(correlation_id, success).build_checked()
    }

    /// Build a checked typed IPC progress handoff.
    pub fn ipc_progress(correlation_id: u64) -> Result<Self> {
        CommandIpcHandoffBuilder::ipc_progress(correlation_id).build_checked()
    }

    /// Build a checked typed IPC cancel handoff.
    pub fn ipc_cancel(correlation_id: u64) -> Result<Self> {
        CommandIpcHandoffBuilder::ipc_cancel(correlation_id).build_checked()
    }

    /// Build a checked extension RPC handoff.
    pub fn extension_rpc(message_kind: impl Into<String>) -> Result<Self> {
        CommandIpcHandoffBuilder::extension_rpc(message_kind).build_checked()
    }

    /// Build a checked hosted bridge handoff.
    pub fn hosted_bridge(message_kind: impl Into<String>) -> Result<Self> {
        CommandIpcHandoffBuilder::hosted_bridge(message_kind).build_checked()
    }

    /// Request carried by this handoff.
    pub fn request(&self) -> &CommandIpcRequest {
        &self.request
    }

    /// Next action to take.
    pub fn next_action(&self) -> CommandIpcNextAction {
        self.next_action
    }

    /// Whether this handoff registers an app command.
    pub fn is_register_command(&self) -> bool {
        self.request.is_register_command()
    }

    /// Whether this handoff publishes a palette descriptor.
    pub fn is_palette_descriptor(&self) -> bool {
        self.request.is_palette_descriptor()
    }

    /// Whether this handoff sends a typed IPC request.
    pub fn is_ipc_request(&self) -> bool {
        self.request.is_ipc_request()
    }

    /// Whether this handoff sends a typed IPC response.
    pub fn is_ipc_response(&self) -> bool {
        self.request.is_ipc_response()
    }

    /// Whether this handoff sends typed IPC progress.
    pub fn is_ipc_progress(&self) -> bool {
        self.request.is_ipc_progress()
    }

    /// Whether this handoff sends typed IPC cancellation.
    pub fn is_ipc_cancel(&self) -> bool {
        self.request.is_ipc_cancel()
    }

    /// Whether this handoff routes extension RPC traffic.
    pub fn is_extension_rpc(&self) -> bool {
        self.request.is_extension_rpc()
    }

    /// Whether this handoff uses a hosted bridge.
    pub fn is_hosted_bridge(&self) -> bool {
        self.request.is_hosted_bridge()
    }

    /// Command identifier when present.
    pub fn command_id(&self) -> Option<&str> {
        self.request.command_id()
    }

    /// Command name when present.
    pub fn command_name(&self) -> Option<&str> {
        self.request.command_name()
    }

    /// Palette descriptor when present.
    pub fn palette_descriptor_ref(&self) -> Option<&CommandDescriptor> {
        self.request.palette_descriptor()
    }

    /// Content-safe handoff summary.
    pub fn to_text(&self) -> String {
        format!("command ipc handoff: {}", self.request.to_text())
    }
}

fn validate_command_id(id: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!id.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        id == id.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(id.len() <= 128, "{label} cannot be longer than 128 bytes");
    anyhow::ensure!(
        id.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '-' | '_')),
        "{label} must contain only ASCII letters, numbers, '.', ':', '-' or '_'"
    );
    Ok(())
}

fn validate_command_text(value: &str, label: &str, max_len: usize) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.len() <= max_len,
        "{label} cannot be longer than {max_len} bytes"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

fn validate_command_descriptor(descriptor: &CommandDescriptor) -> Result<()> {
    validate_command_id(descriptor.id.as_str(), "command descriptor id")?;
    validate_command_text(&descriptor.label, "command descriptor label", 128)?;
    validate_command_text(&descriptor.category, "command descriptor category", 128)?;
    if let Some(keybinding) = &descriptor.keybinding {
        validate_command_text(keybinding, "command descriptor keybinding", 64)?;
    }
    if let Some(icon) = &descriptor.icon {
        validate_command_id(icon, "command descriptor icon")?;
    }
    Ok(())
}

fn validate_correlation_id(correlation_id: u64) -> Result<()> {
    anyhow::ensure!(
        correlation_id > 0,
        "ipc correlation id must be greater than zero"
    );
    Ok(())
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
        assert!(
            palette
                .register(make_descriptor("file.save", "Save Again", "File"))
                .is_err()
        );
    }

    #[test]
    fn direct_registration_validates_and_bounds_palette_state() {
        let mut palette = CommandPalette::new();
        assert!(
            palette
                .register(make_descriptor("bad id", "Bad", "Tools"))
                .is_err()
        );
        assert!(palette.is_empty());
        assert!(PaletteCommandId::new_checked("bad id").is_err());
        assert_eq!(
            PaletteCommandId::new_checked("tools.valid")
                .unwrap()
                .as_str(),
            "tools.valid"
        );

        for index in 0..MAX_PALETTE_COMMANDS {
            let id = format!("command.{index}");
            let descriptor = make_descriptor(&id, "Command", "Tools");
            palette.commands.insert(descriptor.id.clone(), descriptor);
        }
        assert!(
            palette
                .register(make_descriptor("command.overflow", "Overflow", "Tools"))
                .is_err()
        );
        assert_eq!(palette.len(), MAX_PALETTE_COMMANDS);
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
    fn search_covers_ids_shortcuts_and_returns_stable_order() {
        let mut palette = CommandPalette::new();
        palette
            .register(CommandDescriptor {
                id: PaletteCommandId::new("zebra.export"),
                label: "Zebra".to_string(),
                category: "Tools".to_string(),
                keybinding: Some("Cmd+Shift+E".to_string()),
                icon: None,
            })
            .unwrap();
        palette
            .register(make_descriptor("alpha.export", "Alpha", "Tools"))
            .unwrap();

        assert_eq!(palette.search("alpha.export").len(), 1);
        assert_eq!(palette.search("cmd+shift+e").len(), 1);
        let labels = palette
            .search("tools")
            .into_iter()
            .map(|descriptor| descriptor.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["Alpha", "Zebra"]);
        assert!(palette.search_checked("bad\nquery").is_err());
        assert!(palette.search("bad\nquery").is_empty());
        assert!(
            palette
                .search(&"x".repeat(MAX_PALETTE_QUERY_BYTES + 1))
                .is_empty()
        );
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
        assert!(palette.commands_in_category("bad\ncategory").is_empty());
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

    #[test]
    fn command_palette_summary_is_content_safe() {
        let mut palette = CommandPalette::new();
        palette
            .register(CommandDescriptor {
                id: PaletteCommandId::new("private.customer.export"),
                label: "Export Secret Customer List".to_string(),
                category: "Private Operations".to_string(),
                keybinding: Some("Cmd+Shift+E".to_string()),
                icon: Some("confidential-icon".to_string()),
            })
            .unwrap();
        palette
            .register(make_descriptor(
                "private.customer.delete",
                "Delete Secret Customer",
                "Private Operations",
            ))
            .unwrap();

        let id = PaletteCommandId::new("private.customer.export");
        assert_eq!(id.len_bytes(), "private.customer.export".len());
        assert!(!id.is_empty());
        assert!(!id.to_text().contains("private.customer"));

        let descriptor = palette.get(&id).unwrap();
        assert_eq!(descriptor.id_len_bytes(), "private.customer.export".len());
        assert_eq!(
            descriptor.label_len_bytes(),
            "Export Secret Customer List".len()
        );
        assert_eq!(descriptor.category_len_bytes(), "Private Operations".len());
        assert!(descriptor.has_keybinding());
        assert!(descriptor.has_icon());

        let descriptor_summary = descriptor.to_text();
        assert!(descriptor_summary.contains("has_keybinding=true"));
        assert!(descriptor_summary.contains("has_icon=true"));
        assert!(!descriptor_summary.contains("private.customer"));
        assert!(!descriptor_summary.contains("Secret Customer"));
        assert!(!descriptor_summary.contains("Private Operations"));
        assert!(!descriptor_summary.contains("Cmd+Shift+E"));
        assert!(!descriptor_summary.contains("confidential-icon"));

        assert_eq!(palette.len(), 2);
        assert_eq!(palette.category_count(), 1);
        assert_eq!(palette.keybinding_count(), 1);
        assert_eq!(palette.icon_count(), 1);
        let palette_summary = palette.to_text();
        assert_eq!(
            palette_summary,
            "command_palette(commands=2, categories=1, keybindings=1, icons=1, empty=false)"
        );
        assert!(!palette_summary.contains("Secret"));
        assert!(!palette_summary.contains("private.customer"));
    }

    #[test]
    fn command_ipc_handoff_guides_command_palette_ipc_and_bridge_routing() {
        let registration = CommandIpcHandoffBuilder::register_command(
            "private.customer.export",
            "Export Secret Customer List",
        );
        assert_eq!(
            registration.next_action(),
            CommandIpcNextAction::RegisterCommand
        );
        assert_eq!(
            registration.to_text(),
            "command ipc handoff builder: command ipc request: action register-command, correlation false, command registration: id_len_bytes 23, name_len_bytes 27"
        );
        assert!(!registration.to_text().contains("private.customer"));
        assert!(!registration.to_text().contains("Secret Customer"));

        let registration = registration.build_checked().unwrap();
        assert!(registration.is_register_command());
        assert_eq!(
            registration.next_action(),
            CommandIpcNextAction::RegisterCommand
        );
        assert_eq!(registration.command_id(), Some("private.customer.export"));
        assert_eq!(
            registration.command_name(),
            Some("Export Secret Customer List")
        );

        let descriptor = CommandDescriptor {
            id: PaletteCommandId::new("private.customer.delete"),
            label: "Delete Secret Customer".to_string(),
            category: "Private Operations".to_string(),
            keybinding: Some("Cmd+Shift+D".to_string()),
            icon: Some("delete.icon".to_string()),
        };
        let palette = CommandIpcHandoff::palette_descriptor(descriptor).unwrap();
        assert!(palette.is_palette_descriptor());
        assert_eq!(
            palette.next_action(),
            CommandIpcNextAction::PublishPaletteDescriptor
        );
        assert!(palette.palette_descriptor_ref().unwrap().has_keybinding());
        assert!(!palette.to_text().contains("private.customer"));
        assert!(!palette.to_text().contains("Secret Customer"));
        assert!(!palette.to_text().contains("Cmd+Shift+D"));

        let request = CommandIpcHandoff::ipc_request(42).unwrap();
        assert!(request.is_ipc_request());
        assert_eq!(request.next_action(), CommandIpcNextAction::SendIpcRequest);
        assert!(!request.to_text().contains("42"));

        let response = CommandIpcHandoff::ipc_response(42, false).unwrap();
        assert!(response.is_ipc_response());
        assert_eq!(
            response.next_action(),
            CommandIpcNextAction::SendIpcResponse
        );
        assert!(response.to_text().contains("success false"));
        assert!(!response.to_text().contains("42"));

        let progress = CommandIpcHandoff::ipc_progress(42).unwrap();
        assert!(progress.is_ipc_progress());
        assert_eq!(
            progress.next_action(),
            CommandIpcNextAction::SendIpcProgress
        );

        let cancel = CommandIpcHandoff::ipc_cancel(42).unwrap();
        assert!(cancel.is_ipc_cancel());
        assert_eq!(cancel.next_action(), CommandIpcNextAction::SendIpcCancel);

        let extension = CommandIpcHandoff::extension_rpc("settings-changed").unwrap();
        assert!(extension.is_extension_rpc());
        assert_eq!(
            extension.next_action(),
            CommandIpcNextAction::RouteExtensionRpc
        );
        assert!(!extension.to_text().contains("settings-changed"));

        let hosted = CommandIpcHandoff::hosted_bridge("editor-event").unwrap();
        assert!(hosted.is_hosted_bridge());
        assert_eq!(hosted.next_action(), CommandIpcNextAction::UseHostedBridge);
        assert_eq!(
            CommandIpcNextAction::UseHostedBridge.key(),
            "use-hosted-bridge"
        );
        assert!(!hosted.to_text().contains("editor-event"));
    }

    #[test]
    fn command_ipc_handoff_rejects_invalid_generated_requests() {
        assert!(CommandIpcHandoff::register_command("", "Save").is_err());
        assert!(CommandIpcHandoff::register_command(" editor.save", "Save").is_err());
        assert!(CommandIpcHandoff::register_command("editor save", "Save").is_err());
        assert!(CommandIpcHandoff::register_command("editor.save", " Save").is_err());
        assert!(
            CommandIpcHandoff::palette_descriptor(CommandDescriptor {
                id: PaletteCommandId::new("bad id"),
                label: "Bad".to_string(),
                category: "Tools".to_string(),
                keybinding: None,
                icon: None,
            })
            .is_err()
        );
        assert!(
            CommandIpcHandoff::palette_descriptor(CommandDescriptor {
                id: PaletteCommandId::new("tools.bad"),
                label: "Bad".to_string(),
                category: "Tools".to_string(),
                keybinding: Some(" Cmd+B".to_string()),
                icon: None,
            })
            .is_err()
        );
        assert!(CommandIpcHandoff::ipc_request(0).is_err());
        assert!(CommandIpcHandoff::ipc_response(0, true).is_err());
        assert!(CommandIpcHandoff::ipc_progress(0).is_err());
        assert!(CommandIpcHandoff::ipc_cancel(0).is_err());
        assert!(CommandIpcHandoff::extension_rpc("").is_err());
        assert!(CommandIpcHandoff::hosted_bridge("bad\nkind").is_err());
    }
}
