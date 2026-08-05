#![deny(missing_docs)]

//! Release metadata, signed updates, and atomic installation for Kael apps.
//!
//! This crate contains the small, platform-aware contracts used to prepare and
//! install a native application release. It validates [`metadata::AppMetadata`]
//! and [`profile::ReleaseProfile`] values, signs and verifies
//! [`update::UpdateManifest`] values, and applies staged replacements through
//! [`apply::atomic_swap_with_rollback`]. License, SBOM, and notarization models
//! support the surrounding release pipeline.
//!
//! Network retrieval is intentionally outside this crate. Kael's opt-in
//! `auto-update` feature downloads artifacts, enforces the signed size and
//! SHA-256 digest, then delegates the final same-directory swap here.
//!
//! # Signed update metadata
//!
//! ```
//! use kael_release::update::{UpdateChannel, UpdateManifest, UpdatePolicy};
//!
//! let policy = UpdatePolicy::default_stable();
//! policy.validate()?;
//!
//! let manifest = UpdateManifest {
//!     version: "1.1.0".into(),
//!     channel: UpdateChannel::Stable,
//!     url: "https://downloads.example.com/app/1.1.0.tar.gz".into(),
//!     sha256: "a".repeat(64),
//!     size_bytes: 42_000_000,
//!     release_notes: Some("Performance and reliability improvements".into()),
//!     min_version: Some("1.0.0".into()),
//! };
//! manifest.validate()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

/// Re-export of the `ed25519-dalek` crate so downstream tools can construct
/// signatures and keys against the exact version used for manifest signing.
pub use ed25519_dalek;

pub mod apply;
pub mod license;
pub mod metadata;
pub mod notarize;
pub mod profile;
pub mod update;
