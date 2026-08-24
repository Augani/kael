//! Typed failures for portable share operations.

/// A checked share failure that applications can handle without parsing text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShareError {
    /// The share payload failed Kael's bounded validation.
    #[error("invalid share payload: {0}")]
    InvalidPayload(String),
    /// The current target does not expose a system share API.
    #[error("the system share API is unavailable on this platform")]
    Unavailable,
    /// The browser requires this call to run directly from a user activation.
    #[error("browser sharing requires an active user gesture")]
    UserActivationRequired,
    /// The selected backend cannot represent part of the payload.
    #[error("the share payload is unsupported by this backend: {0}")]
    UnsupportedPayload(String),
    /// The user dismissed the share picker before completing the operation.
    #[error("the share operation was cancelled")]
    Cancelled,
    /// Browser or host policy rejected the operation.
    #[error("the share operation was denied by browser or host policy")]
    PermissionDenied,
    /// A platform backend failed after accepting a valid request.
    #[error("the share backend failed: {0}")]
    Platform(String),
}

impl ShareError {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn platform(error: impl std::fmt::Display) -> Self {
        Self::Platform(error.to_string())
    }
}

/// Result returned by the typed portable share API.
pub type ShareOperationResult<T> = std::result::Result<T, ShareError>;
