use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::{
    action::NotificationAction,
    local::{LocalNotification, NotificationPayload},
};

use super::{NotificationBackend, PlatformNotificationSupport};

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "mac-notification-sys",
    action_backend: "mac-notification-sys-actions",
    push_backend: "not-implemented",
};

impl NotificationBackend for PlatformBackend {
    fn deliver(
        &self,
        payload: &NotificationPayload,
        notification: &LocalNotification,
        actions: &[NotificationAction],
        on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
    ) -> Result<()> {
        if actions.is_empty() {
            let mut options = mac_notification_sys::Notification::new();
            configure_sound(&mut options, notification.sound.as_ref());
            mac_notification_sys::send_notification(
                &payload.title,
                notification.subtitle.as_deref(),
                &notification.body,
                Some(&options),
            )
            .map_err(|error| anyhow!("failed to show notification: {error}"))?;
            return Ok(());
        }

        let title = payload.title.clone();
        let subtitle = notification.subtitle.clone();
        let body = notification.body.clone();
        let sound = notification.sound.clone();
        let actions = actions.to_vec();
        std::thread::Builder::new()
            .name("kael-notification-actions".into())
            .spawn(move || {
                let labels = actions
                    .iter()
                    .map(|action| action.title.as_str())
                    .collect::<Vec<_>>();
                let mut options = mac_notification_sys::Notification::new();
                configure_sound(&mut options, sound.as_ref());
                if labels.len() == 1 {
                    options.main_button(mac_notification_sys::MainButton::SingleAction(labels[0]));
                } else {
                    options.main_button(mac_notification_sys::MainButton::DropdownActions(
                        "Actions", &labels,
                    ));
                }

                match mac_notification_sys::send_notification(
                    &title,
                    subtitle.as_deref(),
                    &body,
                    Some(&options),
                ) {
                    Ok(mac_notification_sys::NotificationResponse::ActionButton(label)) => {
                        if let Some(action) = actions.iter().find(|action| action.title == label)
                            && let Some(callback) = on_action.as_ref()
                        {
                            callback(action.identifier.clone());
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("kael notification action delivery failed: {error}");
                    }
                }
            })
            .map_err(|error| {
                anyhow!("failed to start macOS notification action listener: {error}")
            })?;
        Ok(())
    }
}

fn configure_sound(
    options: &mut mac_notification_sys::Notification<'_>,
    sound: Option<&crate::local::NotificationSound>,
) {
    match sound {
        Some(crate::local::NotificationSound::Default) => {
            options.default_sound();
        }
        Some(crate::local::NotificationSound::Named(name)) => {
            options.sound(name);
        }
        Some(crate::local::NotificationSound::Silent) | None => {}
    }
}
