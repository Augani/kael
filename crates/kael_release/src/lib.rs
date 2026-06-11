#![deny(missing_docs)]

//! Packaging, updates, and release hardening for Kael applications.

/// Re-export of the `ed25519-dalek` crate so downstream tools can construct
/// signatures and keys against the exact version used for manifest signing.
pub use ed25519_dalek;

pub mod license;
pub mod metadata;
pub mod notarize;
pub mod profile;
pub mod update;
