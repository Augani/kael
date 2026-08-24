//! Platform metadata and delivery backends.

use std::{future::Future, pin::Pin, sync::Arc};

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
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(target_arch = "wasm32")]
use web as imp;
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
    /// Whether permission is decided through an asynchronous prompt.
    pub asynchronous_permission: bool,
    /// Whether immediate notification delivery is available.
    pub immediate_delivery: bool,
    /// Whether in-process interval triggers are available.
    pub interval_triggers: bool,
    /// Whether calendar or location triggers are available.
    pub system_triggers: bool,
    /// Whether application-defined action buttons are available.
    pub actions: bool,
    /// Whether delivered notifications can be closed by identifier.
    pub cancellation: bool,
}

/// Returns the notification support metadata for the current target.
pub const fn support() -> PlatformNotificationSupport {
    imp::SUPPORT
}

/// Returns the current notification permission state without opening a prompt.
pub fn permission_status() -> crate::NotificationPermissionStatus {
    #[cfg(target_arch = "wasm32")]
    {
        imp::permission_status()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::NotificationPermissionStatus::PlatformManaged
    }
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

    fn request_authorization_async<'a>(
        &'a self,
        options: &'a AuthorizationOptions,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + 'a>> {
        let result = self.request_authorization(options);
        Box::pin(async move { result })
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

    fn cancel(&self, _id: crate::NotificationId) {}

    fn cancel_all(&self) {}
}

pub(crate) fn default_backend() -> Arc<dyn NotificationBackend> {
    Arc::new(imp::PlatformBackend)
}
