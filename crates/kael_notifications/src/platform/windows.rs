use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::{
    action::NotificationAction,
    local::{LocalNotification, NotificationPayload},
};

use super::{NotificationBackend, PlatformNotificationSupport};

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "notify-rust-winrt",
    action_backend: "planned-toast-actions",
    push_backend: "planned-wns",
};

impl NotificationBackend for PlatformBackend {
    fn deliver(
        &self,
        payload: &NotificationPayload,
        notification: &LocalNotification,
        actions: &[NotificationAction],
        _on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
    ) -> Result<()> {
        if !actions.is_empty() {
            return Err(anyhow!(
                "notification actions are not implemented on Windows in kael_notifications yet"
            ));
        }

        notify_rust::Notification::new()
            .summary(&payload.title)
            .body(&body(notification))
            .show()
            .map_err(|error| anyhow!("failed to show notification: {error}"))?;
        Ok(())
    }
}

fn body(notification: &LocalNotification) -> String {
    match notification.subtitle.as_deref() {
        Some(subtitle) if !subtitle.is_empty() => format!("{subtitle}\n{}", notification.body),
        _ => notification.body.clone(),
    }
}
