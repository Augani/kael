//! Runtime worker support for background task execution.

/// Client-side worker runtime interface.
pub mod worker_client;
/// Host-side worker runtime management.
pub mod worker_host;

pub use worker_client::WorkerClient;
pub use worker_host::WorkerHost;
