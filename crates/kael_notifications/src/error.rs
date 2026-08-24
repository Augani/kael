//! Typed failures and permission states for portable notification delivery.

/// The notification authorization state exposed by the current target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationPermissionStatus {
    /// The platform does not expose a notification API.
    Unavailable,
    /// The user has not made a permission decision yet.
    Prompt,
    /// Notification delivery is authorized.
    Granted,
    /// Permission is implicit or managed by the native notification service.
    PlatformManaged,
    /// The user or browser policy denied notification delivery.
    Denied,
}

/// A checked notification failure that callers can handle without parsing text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotificationError {
    /// The notification failed Kael's bounded validation.
    #[error("invalid notification: {0}")]
    InvalidNotification(String),
    /// The platform does not expose a notification API.
    #[error("notifications are unavailable on this platform")]
    Unavailable,
    /// The synchronous API cannot open the browser permission prompt.
    #[error("notification permission must be requested through the async API")]
    PermissionPromptRequired,
    /// The browser requires a permission prompt to begin during a user activation.
    #[error("browser notification permission requires an active user gesture")]
    UserActivationRequired,
    /// The user or browser policy denied notification delivery.
    #[error("notification permission was denied")]
    PermissionDenied,
    /// The current target cannot provide the requested trigger semantics.
    #[error("notification trigger is unsupported on this target: {0}")]
    UnsupportedTrigger(&'static str),
    /// The current target cannot represent a requested notification feature.
    #[error("notification feature is unsupported on this target: {0}")]
    UnsupportedFeature(&'static str),
    /// The operation was cancelled before delivery.
    #[error("notification delivery was cancelled")]
    Cancelled,
    /// The platform backend failed after accepting a valid request.
    #[error("notification backend failed: {0}")]
    Platform(String),
}

impl NotificationError {
    pub(crate) fn from_anyhow(error: anyhow::Error) -> Self {
        match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => Self::Platform(error.to_string()),
        }
    }
}

/// Result returned by typed portable notification operations.
pub type NotificationOperationResult<T> = std::result::Result<T, NotificationError>;
