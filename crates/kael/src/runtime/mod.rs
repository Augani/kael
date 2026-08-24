//! Runtime worker support for background task execution.

pub(crate) mod web_protocol;

pub use web_protocol::{BROWSER_WORKER_PROTOCOL_VERSION, MAX_BROWSER_WORKER_PENDING_REQUESTS};

/// Client-side worker runtime interface.
#[cfg(not(target_arch = "wasm32"))]
pub mod worker_client;
/// Host-side worker runtime management.
#[cfg(not(target_arch = "wasm32"))]
pub mod worker_host;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use web::{WorkerClient, WorkerHost};
#[cfg(not(target_arch = "wasm32"))]
pub use worker_client::WorkerClient;
#[cfg(not(target_arch = "wasm32"))]
pub use worker_host::WorkerHost;
