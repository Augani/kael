#![deny(missing_docs)]

//! Transport-agnostic networking, sync, and collaboration primitives for Kael.

/// Auth token storage for service credentials.
pub mod auth;
/// Typed, transport-agnostic API request and response models.
pub mod client;
/// Offline request queue for deferred execution.
pub mod offline;
/// Collaboration presence tracking for multi-user sessions.
pub mod presence;
/// Retry policy with exponential backoff.
pub mod retry;
/// Bounded WebSocket transport for native and browser applications.
pub mod websocket;

pub use auth::{AuthToken, SecureTokenStore, TokenStore};
pub use client::{ApiRequest, ApiResponse, HttpMethod};
pub use offline::{EnqueueOutcome, OfflineQueue, QueuedRequest};
pub use presence::{Presence, PresenceStatus, PresenceTracker};
pub use retry::RetryPolicy;
pub use websocket::{
    AllowAllWebSocketHosts, DenyAllWebSocketHosts, WebSocketClient, WebSocketClose,
    WebSocketCloseError, WebSocketCloseMetadata, WebSocketConfig, WebSocketConfigBuilder,
    WebSocketConnectError, WebSocketErrorKind, WebSocketErrorMetadata, WebSocketEvent,
    WebSocketEventKind, WebSocketHostPolicy, WebSocketMessage, WebSocketOpenMetadata,
    WebSocketReconnectMetadata, WebSocketReconnectPolicy, WebSocketSendError, WebSocketState,
};
