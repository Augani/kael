use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::{
    action::NotificationAction,
    local::{LocalNotification, NotificationPayload},
};

use super::{NotificationBackend, PlatformNotificationSupport};

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "notify-rust-dbus",
    action_backend: "notify-rust-wait-for-action",
    push_backend: "custom-server-planned",
};

impl NotificationBackend for PlatformBackend {
    fn deliver(
        &self,
        payload: &NotificationPayload,
        notification: &LocalNotification,
        actions: &[NotificationAction],
        on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
    ) -> Result<()> {
        let mut builder = notify_rust::Notification::new();
        builder.summary(&payload.title).body(&body(notification));
        for action in actions {
            builder.action(&action.identifier, &action.title);
        }
        let handle = builder
            .show()
            .map_err(|error| anyhow!("failed to show notification: {error}"))?;
        if !actions.is_empty() {
            if let Some(on_action) = on_action {
                std::thread::spawn(move || {
                    handle.wait_for_action(|action_id| {
                        if action_id != "__closed" {
                            on_action(action_id.to_string());
                        }
                    });
                });
            }
        }
        Ok(())
    }
}

fn body(notification: &LocalNotification) -> String {
    match notification.subtitle.as_deref() {
        Some(subtitle) if !subtitle.is_empty() => format!("{subtitle}\n{}", notification.body),
        _ => notification.body.clone(),
    }
}