use std::{collections::HashMap, sync::Arc};

use anyhow::{Context as _, Result};
use zbus::{
    blocking::{Connection, Proxy, proxy::SignalIterator},
    zvariant::Value,
};

use crate::{
    action::NotificationAction,
    local::{LocalNotification, NotificationPayload},
};

use super::{NotificationBackend, PlatformNotificationSupport};

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "freedesktop-notifications-dbus",
    action_backend: "freedesktop-action-invoked",
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
        let body = body(notification);
        if actions.is_empty() || on_action.is_none() {
            let connection =
                Connection::session().context("failed to connect to the D-Bus session")?;
            let proxy = notification_proxy(&connection)?;
            send_notification(&proxy, &payload.title, &body, &notification.sound, actions)?;
            return Ok(());
        }

        let title = payload.title.clone();
        let sound = notification.sound.clone();
        let actions = actions.to_vec();
        let Some(on_action) = on_action else {
            return Ok(());
        };
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("kael-notification-actions".into())
            .spawn(
                move || match prepare_action_listener(&title, &body, &sound, &actions) {
                    Ok((_proxy, mut signals, notification_id)) => {
                        let _ = ready_tx.send(Ok(()));
                        if let Err(error) = forward_action(&mut signals, notification_id, on_action)
                        {
                            eprintln!("kael notification action listener failed: {error:#}");
                        }
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("{error:#}")));
                    }
                },
            )
            .context("failed to start the notification action listener")?;
        let delivery = ready_rx
            .recv()
            .context("notification action listener stopped before delivery")?;
        delivery.map_err(anyhow::Error::msg)?;
        Ok(())
    }
}

fn notification_proxy(connection: &Connection) -> Result<Proxy<'_>> {
    Proxy::new(
        connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .context("failed to connect to the freedesktop notification service")
}

fn owned_notification_proxy(connection: Connection) -> Result<Proxy<'static>> {
    Proxy::new_owned(
        connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .context("failed to connect to the freedesktop notification service")
}

fn send_notification(
    proxy: &Proxy<'_>,
    title: &str,
    body: &str,
    sound: &Option<crate::local::NotificationSound>,
    actions: &[NotificationAction],
) -> Result<u32> {
    let actions = actions
        .iter()
        .flat_map(|action| [action.identifier.as_str(), action.title.as_str()])
        .collect::<Vec<_>>();
    let mut hints = HashMap::<&str, Value<'_>>::new();
    match sound {
        Some(crate::local::NotificationSound::Named(name)) => {
            hints.insert("sound-name", Value::Str(name.as_str().into()));
        }
        Some(crate::local::NotificationSound::Silent) => {
            hints.insert("suppress-sound", Value::Bool(true));
        }
        Some(crate::local::NotificationSound::Default) | None => {}
    }
    proxy
        .call(
            "Notify",
            &("Kael", 0_u32, "", title, body, actions, hints, -1_i32),
        )
        .context("failed to show notification")
}

fn prepare_action_listener(
    title: &str,
    body: &str,
    sound: &Option<crate::local::NotificationSound>,
    actions: &[NotificationAction],
) -> Result<(Proxy<'static>, SignalIterator<'static>, u32)> {
    let connection = Connection::session().context("failed to connect to the D-Bus session")?;
    let proxy = owned_notification_proxy(connection)?;
    let signals = proxy
        .receive_all_signals()
        .context("failed to subscribe to notification lifecycle events")?;
    let notification_id = send_notification(&proxy, title, body, sound, actions)?;
    Ok((proxy, signals, notification_id))
}

fn forward_action(
    signals: &mut SignalIterator<'_>,
    notification_id: u32,
    on_action: Arc<dyn Fn(String) + Send + Sync + 'static>,
) -> Result<()> {
    for message in signals {
        match message.header().member().map(|member| member.as_str()) {
            Some("ActionInvoked") => {
                let (id, action_id): (u32, String) = message
                    .body()
                    .deserialize()
                    .context("invalid notification action signal")?;
                if id == notification_id {
                    if action_id != "__closed" {
                        on_action(action_id);
                    }
                    return Ok(());
                }
            }
            Some("NotificationClosed") => {
                let (id, _reason): (u32, u32) = message
                    .body()
                    .deserialize()
                    .context("invalid notification close signal")?;
                if id == notification_id {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn body(notification: &LocalNotification) -> String {
    match notification.subtitle.as_deref() {
        Some(subtitle) if !subtitle.is_empty() => format!("{subtitle}\n{}", notification.body),
        _ => notification.body.clone(),
    }
}
