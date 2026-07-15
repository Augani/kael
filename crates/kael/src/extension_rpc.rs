//! Extension RPC contract and typed transport wrappers.

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    ipc_transport::{Transport, decode_exact_frame, encode_frame},
    plugin::Contributions,
    process_model::IpcMessage,
};

/// RPC protocol version for extensions.
pub const EXTENSION_RPC_VERSION: u32 = 1;
const MAX_EXTENSION_RPC_ID_BYTES: usize = 128;
const MAX_EXTENSION_RPC_ERROR_BYTES: usize = 4 * 1024;
const MAX_EXTENSION_CAPABILITIES: usize = 64;

fn validate_rpc_id(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have surrounding whitespace"
    );
    anyhow::ensure!(
        value.len() <= MAX_EXTENSION_RPC_ID_BYTES,
        "{label} cannot exceed {MAX_EXTENSION_RPC_ID_BYTES} bytes"
    );
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '-' | '_' | '/')),
        "{label} contains unsupported characters"
    );
    Ok(())
}

fn validate_rpc_error(error: &str) -> Result<()> {
    anyhow::ensure!(
        error.len() <= MAX_EXTENSION_RPC_ERROR_BYTES,
        "extension RPC error cannot exceed {MAX_EXTENSION_RPC_ERROR_BYTES} bytes"
    );
    Ok(())
}

/// Request sent from the host to an extension process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionRequest {
    /// Activate the extension.
    Activate,
    /// Deactivate the extension.
    Deactivate,
    /// Execute a contributed command.
    ExecuteCommand {
        /// Command identifier.
        command_id: String,
        /// Arguments as JSON.
        args: Option<serde_json::Value>,
    },
    /// Query the extension for its current contributions.
    GetContributions,
    /// Shut down the extension process.
    Shutdown,
}

impl ExtensionRequest {
    /// Validate request fields before crossing the extension boundary.
    pub fn validate(&self) -> Result<()> {
        if let Self::ExecuteCommand { command_id, .. } = self {
            validate_rpc_id(command_id, "extension command id")?;
        }
        Ok(())
    }

    /// Stable extension request kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::Deactivate => "deactivate",
            Self::ExecuteCommand { .. } => "execute-command",
            Self::GetContributions => "get-contributions",
            Self::Shutdown => "shutdown",
        }
    }

    /// Byte length of the command id without exposing it.
    pub fn command_id_len_bytes(&self) -> usize {
        match self {
            Self::ExecuteCommand { command_id, .. } => command_id.len(),
            _ => 0,
        }
    }

    /// Returns true when command arguments are present.
    pub fn has_args(&self) -> bool {
        matches!(self, Self::ExecuteCommand { args: Some(_), .. })
    }

    /// Coarse JSON args kind, or `none`.
    pub fn args_kind(&self) -> &'static str {
        match self {
            Self::ExecuteCommand {
                args: Some(args), ..
            } => json_value_kind(args),
            _ => "none",
        }
    }

    /// Number of top-level JSON arg items for arrays/objects.
    pub fn args_item_count(&self) -> usize {
        match self {
            Self::ExecuteCommand {
                args: Some(args), ..
            } => json_value_item_count(args),
            _ => 0,
        }
    }

    /// Content-safe extension request summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_request(kind={}, command_id_len_bytes={}, has_args={}, args_kind={}, args_items={})",
            self.kind(),
            self.command_id_len_bytes(),
            self.has_args(),
            self.args_kind(),
            self.args_item_count()
        )
    }
}

/// Response sent from an extension process to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionResponse {
    /// Simple acknowledgment.
    Ack,
    /// Contribution data.
    Contributions(Contributions),
    /// Error response.
    Error(String),
}

impl ExtensionResponse {
    /// Validate response fields received from an extension.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Ack => Ok(()),
            Self::Contributions(contributions) => contributions.validate(),
            Self::Error(error) => validate_rpc_error(error),
        }
    }

    /// Stable extension response kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Ack => "ack",
            Self::Contributions(_) => "contributions",
            Self::Error(_) => "error",
        }
    }

    /// Returns true when contribution data is included.
    pub fn has_contributions(&self) -> bool {
        matches!(self, Self::Contributions(_))
    }

    /// Byte length of the error message without exposing it.
    pub fn error_len_bytes(&self) -> usize {
        match self {
            Self::Error(error) => error.len(),
            _ => 0,
        }
    }

    /// Content-safe extension response summary.
    pub fn to_text(&self) -> String {
        let contribution_summary = match self {
            Self::Contributions(contributions) => contributions.to_text(),
            _ => "none".to_string(),
        };
        format!(
            "extension_response(kind={}, has_contributions={}, contributions={}, error_len_bytes={})",
            self.kind(),
            self.has_contributions(),
            contribution_summary,
            self.error_len_bytes()
        )
    }
}

/// Notification sent from an extension process to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionNotification {
    /// A command was executed by the extension.
    CommandExecuted {
        /// The command identifier.
        command_id: String,
        /// Result as JSON.
        result: Option<serde_json::Value>,
    },
    /// A panel state was updated.
    PanelUpdated {
        /// The panel identifier.
        panel_id: String,
        /// Updated state as JSON.
        state: Option<serde_json::Value>,
    },
    /// A setting value was changed.
    SettingsChanged {
        /// The setting key.
        key: String,
        /// The new value.
        value: serde_json::Value,
    },
}

impl ExtensionNotification {
    /// Validate notification fields before dispatch.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::CommandExecuted { command_id, .. } => {
                validate_rpc_id(command_id, "extension command id")
            }
            Self::PanelUpdated { panel_id, .. } => validate_rpc_id(panel_id, "extension panel id"),
            Self::SettingsChanged { key, .. } => validate_rpc_id(key, "extension setting key"),
        }
    }

    /// Stable extension notification kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::CommandExecuted { .. } => "command-executed",
            Self::PanelUpdated { .. } => "panel-updated",
            Self::SettingsChanged { .. } => "settings-changed",
        }
    }

    /// Byte length of the affected command, panel, or setting id without exposing it.
    pub fn target_len_bytes(&self) -> usize {
        match self {
            Self::CommandExecuted { command_id, .. } => command_id.len(),
            Self::PanelUpdated { panel_id, .. } => panel_id.len(),
            Self::SettingsChanged { key, .. } => key.len(),
        }
    }

    /// Returns true when the notification includes a JSON payload.
    pub fn has_payload(&self) -> bool {
        match self {
            Self::CommandExecuted { result, .. } => result.is_some(),
            Self::PanelUpdated { state, .. } => state.is_some(),
            Self::SettingsChanged { .. } => true,
        }
    }

    /// Coarse JSON payload kind, or `none`.
    pub fn payload_kind(&self) -> &'static str {
        match self {
            Self::CommandExecuted {
                result: Some(result),
                ..
            } => json_value_kind(result),
            Self::PanelUpdated {
                state: Some(state), ..
            } => json_value_kind(state),
            Self::SettingsChanged { value, .. } => json_value_kind(value),
            _ => "none",
        }
    }

    /// Number of top-level JSON payload items for arrays/objects.
    pub fn payload_item_count(&self) -> usize {
        match self {
            Self::CommandExecuted {
                result: Some(result),
                ..
            } => json_value_item_count(result),
            Self::PanelUpdated {
                state: Some(state), ..
            } => json_value_item_count(state),
            Self::SettingsChanged { value, .. } => json_value_item_count(value),
            _ => 0,
        }
    }

    /// Content-safe extension notification summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_notification(kind={}, target_len_bytes={}, has_payload={}, payload_kind={}, payload_items={})",
            self.kind(),
            self.target_len_bytes(),
            self.has_payload(),
            self.payload_kind(),
            self.payload_item_count()
        )
    }
}

/// Handshake messages exchanged during extension initialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionHandshake {
    /// Host sends RPC version and granted capabilities.
    Host {
        /// Protocol version.
        version: u32,
        /// Granted capabilities as JSON values.
        capabilities: Vec<serde_json::Value>,
    },
    /// Extension acknowledges with its supported version.
    Extension {
        /// Extension protocol version.
        version: u32,
        /// Whether the extension accepts the handshake.
        accepted: bool,
    },
}

impl ExtensionHandshake {
    /// Validate handshake fields received across the process boundary.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.version() > 0, "extension RPC version must be non-zero");
        if let Self::Host { capabilities, .. } = self {
            anyhow::ensure!(
                capabilities.len() <= MAX_EXTENSION_CAPABILITIES,
                "extension handshake cannot include more than {MAX_EXTENSION_CAPABILITIES} capabilities"
            );
        }
        Ok(())
    }

    /// Stable handshake kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Host { .. } => "host",
            Self::Extension { .. } => "extension",
        }
    }

    /// Protocol version.
    pub fn version(&self) -> u32 {
        match self {
            Self::Host { version, .. } | Self::Extension { version, .. } => *version,
        }
    }

    /// Number of capability descriptors.
    pub fn capability_count(&self) -> usize {
        match self {
            Self::Host { capabilities, .. } => capabilities.len(),
            Self::Extension { .. } => 0,
        }
    }

    /// Whether the extension accepted the handshake, if applicable.
    pub fn accepted(&self) -> Option<bool> {
        match self {
            Self::Extension { accepted, .. } => Some(*accepted),
            Self::Host { .. } => None,
        }
    }

    /// Content-safe handshake summary.
    pub fn to_text(&self) -> String {
        format!(
            "extension_handshake(kind={}, version={}, capabilities={}, accepted={})",
            self.kind(),
            self.version(),
            self.capability_count(),
            self.accepted().unwrap_or(false)
        )
    }
}

/// Unified message type for extension communication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtensionMessage {
    /// RPC request or response.
    Rpc(IpcMessage<ExtensionRequest, ExtensionResponse, (), String>),
    /// One-way notification.
    Notification(ExtensionNotification),
    /// Handshake exchange.
    Handshake(ExtensionHandshake),
}

impl ExtensionMessage {
    /// Validate the message payload before use or transmission.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Rpc(IpcMessage::Request { body, .. }) => body.validate(),
            Self::Rpc(IpcMessage::Response { result, .. }) => match result {
                Ok(response) => response.validate(),
                Err(error) => validate_rpc_error(error),
            },
            Self::Rpc(IpcMessage::Progress { .. }) => {
                Err(anyhow!("extension RPC does not support progress messages"))
            }
            Self::Rpc(IpcMessage::Cancel { .. }) => Ok(()),
            Self::Notification(notification) => notification.validate(),
            Self::Handshake(handshake) => handshake.validate(),
        }
    }

    /// Stable extension message kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Rpc(_) => "rpc",
            Self::Notification(_) => "notification",
            Self::Handshake(_) => "handshake",
        }
    }

    /// Content-safe extension message summary.
    pub fn to_text(&self) -> String {
        let detail = match self {
            Self::Rpc(message) => message.to_text(),
            Self::Notification(notification) => notification.to_text(),
            Self::Handshake(handshake) => handshake.to_text(),
        };
        format!("extension_message(kind={}, detail={})", self.kind(), detail)
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn json_value_item_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => items.len(),
        serde_json::Value::Object(items) => items.len(),
        _ => 0,
    }
}

/// Typed transport wrapper for extension communication.
pub struct ExtensionTransport {
    inner: Box<dyn Transport>,
}

impl ExtensionTransport {
    /// Wrap an existing transport.
    pub fn new(inner: Box<dyn Transport>) -> Self {
        Self { inner }
    }

    /// Send an RPC request.
    pub fn send_request(&mut self, id: u64, body: ExtensionRequest) -> Result<()> {
        let msg = ExtensionMessage::Rpc(IpcMessage::Request { id, body });
        msg.validate()?;
        let payload = serde_json::to_vec(&msg).context("failed to serialize request")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Send an RPC response.
    pub fn send_response(
        &mut self,
        id: u64,
        result: Result<ExtensionResponse, String>,
    ) -> Result<()> {
        let msg = ExtensionMessage::Rpc(IpcMessage::Response { id, result });
        msg.validate()?;
        let payload = serde_json::to_vec(&msg).context("failed to serialize response")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Send a one-way notification.
    pub fn send_notification(&mut self, notification: ExtensionNotification) -> Result<()> {
        let msg = ExtensionMessage::Notification(notification);
        msg.validate()?;
        let payload = serde_json::to_vec(&msg).context("failed to serialize notification")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Send a handshake message.
    pub fn send_handshake(&mut self, handshake: ExtensionHandshake) -> Result<()> {
        let msg = ExtensionMessage::Handshake(handshake);
        msg.validate()?;
        let payload = serde_json::to_vec(&msg).context("failed to serialize handshake")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Receive the next message.
    pub fn recv_message(&mut self) -> Result<ExtensionMessage> {
        let frame = self.inner.recv_frame()?;
        let payload = decode_exact_frame(&frame)?;
        let message: ExtensionMessage =
            serde_json::from_slice(&payload).context("failed to deserialize message")?;
        message.validate()?;
        Ok(message)
    }

    /// Unwrap the underlying transport.
    pub fn into_inner(self) -> Box<dyn Transport> {
        self.inner
    }
}

/// RPC client that runs inside the extension process.
pub struct ExtensionRpcClient {
    transport: ExtensionTransport,
}

impl ExtensionRpcClient {
    /// Create a new RPC client.
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport: ExtensionTransport::new(transport),
        }
    }

    /// Receive the next request from the host.
    pub fn recv_request(&mut self) -> Result<(u64, ExtensionRequest)> {
        match self.transport.recv_message()? {
            ExtensionMessage::Rpc(IpcMessage::Request { id, body }) => Ok((id, body)),
            other => Err(anyhow!("unexpected message: {}", other.to_text())),
        }
    }

    /// Send a response to the host.
    pub fn send_response(
        &mut self,
        id: u64,
        result: Result<ExtensionResponse, String>,
    ) -> Result<()> {
        self.transport.send_response(id, result)
    }

    /// Send a notification to the host.
    pub fn send_notification(&mut self, notification: ExtensionNotification) -> Result<()> {
        self.transport.send_notification(notification)
    }
}

/// RPC host that runs in the main application process.
pub struct ExtensionRpcHost {
    transport: ExtensionTransport,
}

impl ExtensionRpcHost {
    /// Create a new RPC host.
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport: ExtensionTransport::new(transport),
        }
    }

    /// Send a request to the extension.
    pub fn send_request(&mut self, id: u64, request: ExtensionRequest) -> Result<()> {
        self.transport.send_request(id, request)
    }

    /// Receive the next response from the extension.
    pub fn recv_response(&mut self) -> Result<(u64, Result<ExtensionResponse, String>)> {
        match self.transport.recv_message()? {
            ExtensionMessage::Rpc(IpcMessage::Response { id, result }) => Ok((id, result)),
            other => Err(anyhow!("unexpected message: {}", other.to_text())),
        }
    }

    /// Receive the next notification from the extension.
    pub fn recv_notification(&mut self) -> Result<ExtensionNotification> {
        match self.transport.recv_message()? {
            ExtensionMessage::Notification(notification) => Ok(notification),
            other => Err(anyhow!("unexpected message: {}", other.to_text())),
        }
    }

    /// Send an acknowledgment response.
    pub fn send_ack(&mut self, id: u64) -> Result<()> {
        self.transport.send_response(id, Ok(ExtensionResponse::Ack))
    }

    /// Send an error response.
    pub fn send_error(&mut self, id: u64, error: String) -> Result<()> {
        self.transport.send_response(id, Err(error))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_transport::InMemoryTransport;

    #[test]
    fn test_extension_rpc_roundtrip() {
        let (ta, tb) = InMemoryTransport::pair();
        let mut host = ExtensionTransport::new(Box::new(ta));
        let mut client = ExtensionTransport::new(Box::new(tb));

        host.send_request(1, ExtensionRequest::GetContributions)
            .unwrap();
        let msg = client.recv_message().unwrap();
        assert!(matches!(
            msg,
            ExtensionMessage::Rpc(IpcMessage::Request {
                id: 1,
                body: ExtensionRequest::GetContributions
            })
        ));

        client.send_response(1, Ok(ExtensionResponse::Ack)).unwrap();
        let msg = host.recv_message().unwrap();
        assert!(matches!(
            msg,
            ExtensionMessage::Rpc(IpcMessage::Response {
                id: 1,
                result: Ok(ExtensionResponse::Ack)
            })
        ));
    }

    #[test]
    fn test_extension_notification_roundtrip() {
        let (ta, tb) = InMemoryTransport::pair();
        let mut host = ExtensionTransport::new(Box::new(ta));
        let mut client = ExtensionTransport::new(Box::new(tb));

        let notification = ExtensionNotification::SettingsChanged {
            key: "theme".to_string(),
            value: serde_json::json!("dark"),
        };
        client.send_notification(notification.clone()).unwrap();
        let received = host.recv_message().unwrap();
        assert_eq!(received, ExtensionMessage::Notification(notification));
    }

    #[test]
    fn test_extension_handshake_roundtrip() {
        let (ta, tb) = InMemoryTransport::pair();
        let mut host = ExtensionTransport::new(Box::new(ta));
        let mut client = ExtensionTransport::new(Box::new(tb));

        let handshake = ExtensionHandshake::Host {
            version: 1,
            capabilities: vec![serde_json::json!("network")],
        };
        host.send_handshake(handshake.clone()).unwrap();
        let received = client.recv_message().unwrap();
        assert_eq!(received, ExtensionMessage::Handshake(handshake));
    }

    #[test]
    fn extension_rpc_rejects_invalid_identifiers_and_payloads() {
        let (host_side, _peer) = InMemoryTransport::pair();
        let mut transport = ExtensionTransport::new(Box::new(host_side));
        assert!(
            transport
                .send_request(
                    1,
                    ExtensionRequest::ExecuteCommand {
                        command_id: "../unsafe command".to_string(),
                        args: None,
                    },
                )
                .is_err()
        );
        assert!(
            transport
                .send_response(1, Err("x".repeat(MAX_EXTENSION_RPC_ERROR_BYTES + 1)))
                .is_err()
        );
        assert!(
            transport
                .send_handshake(ExtensionHandshake::Host {
                    version: 1,
                    capabilities: vec![serde_json::Value::Null; MAX_EXTENSION_CAPABILITIES + 1],
                })
                .is_err()
        );
    }

    #[test]
    fn extension_rpc_summary_is_content_safe() {
        let request = ExtensionRequest::ExecuteCommand {
            command_id: "private.customer.export".to_string(),
            args: Some(serde_json::json!({
                "secret": "customer-token",
                "items": [1, 2, 3]
            })),
        };
        assert_eq!(request.kind(), "execute-command");
        assert_eq!(
            request.command_id_len_bytes(),
            "private.customer.export".len()
        );
        assert!(request.has_args());
        assert_eq!(request.args_kind(), "object");
        assert_eq!(request.args_item_count(), 2);
        let request_summary = request.to_text();
        assert!(request_summary.contains("kind=execute-command"));
        assert!(!request_summary.contains("private.customer"));
        assert!(!request_summary.contains("customer-token"));
        assert!(!request_summary.contains("secret"));

        let response = ExtensionResponse::Error("private extension failed".to_string());
        assert_eq!(response.kind(), "error");
        assert_eq!(response.error_len_bytes(), "private extension failed".len());
        let response_summary = response.to_text();
        assert!(response_summary.contains("kind=error"));
        assert!(!response_summary.contains("private extension failed"));

        let contributions = ExtensionResponse::Contributions(Contributions {
            commands: vec![crate::plugin::ContributedCommand {
                id: "private.command".to_string(),
                title: "Private Command".to_string(),
                keybinding: None,
            }],
            menu_items: Vec::new(),
            panels: Vec::new(),
            settings_schema: None,
        });
        let contributions_summary = contributions.to_text();
        assert!(contributions_summary.contains("has_contributions=true"));
        assert!(contributions_summary.contains("commands=1"));
        assert!(!contributions_summary.contains("private.command"));
        assert!(!contributions_summary.contains("Private Command"));

        let notification = ExtensionNotification::SettingsChanged {
            key: "private.setting".to_string(),
            value: serde_json::json!({
                "secret": "setting-token",
                "enabled": true
            }),
        };
        assert_eq!(notification.kind(), "settings-changed");
        assert_eq!(notification.target_len_bytes(), "private.setting".len());
        assert!(notification.has_payload());
        assert_eq!(notification.payload_kind(), "object");
        assert_eq!(notification.payload_item_count(), 2);
        let notification_summary = notification.to_text();
        assert!(notification_summary.contains("kind=settings-changed"));
        assert!(!notification_summary.contains("private.setting"));
        assert!(!notification_summary.contains("setting-token"));
        assert!(!notification_summary.contains("secret"));

        let handshake = ExtensionHandshake::Host {
            version: EXTENSION_RPC_VERSION,
            capabilities: vec![serde_json::json!("private:network")],
        };
        assert_eq!(handshake.kind(), "host");
        assert_eq!(handshake.capability_count(), 1);
        let handshake_summary = handshake.to_text();
        assert!(handshake_summary.contains("kind=host"));
        assert!(!handshake_summary.contains("private:network"));

        let message = ExtensionMessage::Rpc(IpcMessage::Request {
            id: 99,
            body: request,
        });
        let message_summary = message.to_text();
        assert!(message_summary.contains("kind=rpc"));
        assert!(message_summary.contains("correlation_id=99"));
        assert!(!message_summary.contains("private.customer"));
        assert!(!message_summary.contains("customer-token"));
    }

    #[test]
    fn test_extension_rpc_client_api() {
        let (ta, tb) = InMemoryTransport::pair();
        let mut client = ExtensionRpcClient::new(Box::new(ta));
        let mut host_transport = ExtensionTransport::new(Box::new(tb));

        host_transport
            .send_request(42, ExtensionRequest::Activate)
            .unwrap();
        let (id, req) = client.recv_request().unwrap();
        assert_eq!(id, 42);
        assert_eq!(req, ExtensionRequest::Activate);

        client
            .send_response(42, Ok(ExtensionResponse::Ack))
            .unwrap();
        let (id, result) = ExtensionRpcHost::new(host_transport.into_inner())
            .recv_response()
            .unwrap();
        assert_eq!(id, 42);
        assert_eq!(result, Ok(ExtensionResponse::Ack));
    }
}
