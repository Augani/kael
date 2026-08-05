//! Platform metadata and delivery backends.

use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::{
    action::NotificationAction,
    local::{AuthorizationOptions, LocalNotification, NotificationPayload},
    push::PushToken,
};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// Platform metadata for notification support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformNotificationSupport {
    /// The local-delivery backend.
    pub delivery_backend: &'static str,
    /// The action-handling backend.
    pub action_backend: &'static str,
    /// The push-registration backend.
    pub push_backend: &'static str,
}

/// Returns the notification support metadata for the current target.
pub const fn support() -> PlatformNotificationSupport {
    imp::SUPPORT
}

/// Configure the AppUserModelID used for Windows toast delivery.
///
/// Packaged Windows applications should call this once, before scheduling a
/// notification, with the AppUserModelID registered by their installer. An
/// unpackaged development build uses a shell fallback when no ID is configured.
#[cfg(target_os = "windows")]
pub fn set_windows_app_user_model_id(id: impl Into<String>) -> Result<()> {
    imp::set_app_user_model_id(id.into())
}

pub(crate) trait NotificationBackend: Send + Sync + 'static {
    fn request_authorization(&self, _options: &AuthorizationOptions) -> Result<bool> {
        Ok(true)
    }

    fn deliver(
        &self,
        payload: &NotificationPayload,
        notification: &LocalNotification,
        actions: &[NotificationAction],
        on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
    ) -> Result<()>;

    fn register_for_push(&self) -> Result<PushToken> {
        Err(anyhow!(
            "push registration is not implemented on this platform"
        ))
    }

    fn set_badge_count(&self, _count: u32) -> Result<()> {
        Err(anyhow!(
            "application badge counts are not implemented on this platform"
        ))
    }
}

pub(crate) fn default_backend() -> Arc<dyn NotificationBackend> {
    Arc::new(imp::PlatformBackend)
}
