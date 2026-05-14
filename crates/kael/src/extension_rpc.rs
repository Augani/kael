//! Extension RPC contract and typed transport wrappers.

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    ipc_transport::{Transport, decode_frame, encode_frame},
    plugin::Contributions,
    process_model::IpcMessage,
};

/// RPC protocol version for extensions.
pub const EXTENSION_RPC_VERSION: u32 = 1;

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
        let payload = serde_json::to_vec(&msg).context("failed to serialize request")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Send an RPC response.
    pub fn send_response(
        &mut self,
        id: u64,
        result: Result<ExtensionResponse, String>,
    ) -> Result<()> {
        let msg = ExtensionMessage::Rpc(IpcMessage::Response { id, result });
        let payload = serde_json::to_vec(&msg).context("failed to serialize response")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Send a one-way notification.
    pub fn send_notification(&mut self, notification: ExtensionNotification) -> Result<()> {
        let msg = ExtensionMessage::Notification(notification);
        let payload = serde_json::to_vec(&msg).context("failed to serialize notification")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Send a handshake message.
    pub fn send_handshake(&mut self, handshake: ExtensionHandshake) -> Result<()> {
        let msg = ExtensionMessage::Handshake(handshake);
        let payload = serde_json::to_vec(&msg).context("failed to serialize handshake")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Receive the next message.
    pub fn recv_message(&mut self) -> Result<ExtensionMessage> {
        let frame = self.inner.recv_frame()?;
        let (payload, _) = decode_frame(&frame)?.ok_or_else(|| anyhow!("incomplete frame"))?;
        serde_json::from_slice(&payload).context("failed to deserialize message")
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
            other => Err(anyhow!("unexpected message: {:?}", other)),
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
            other => Err(anyhow!("unexpected message: {:?}", other)),
        }
    }

    /// Receive the next notification from the extension.
    pub fn recv_notification(&mut self) -> Result<ExtensionNotification> {
        match self.transport.recv_message()? {
            ExtensionMessage::Notification(notification) => Ok(notification),
            other => Err(anyhow!("unexpected message: {:?}", other)),
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
