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

pub use auth::{AuthToken, SecureTokenStore, TokenStore};
pub use client::{ApiRequest, ApiResponse, HttpMethod};
pub use offline::{OfflineQueue, QueuedRequest};
pub use presence::{Presence, PresenceStatus, PresenceTracker};
pub use retry::RetryPolicy;
