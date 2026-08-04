use std::{collections::HashMap, sync::Arc};

use anyhow::{Context as _, Result};
use zbus::{
    blocking::{Connection, Proxy, proxy::SignalIterator},
    zvariant::OwnedValue,
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
            send_notification(&proxy, &payload.title, &body, actions)?;
            return Ok(());
        }

        let title = payload.title.clone();
        let actions = actions.to_vec();
        let Some(on_action) = on_action else {
            return Ok(());
        };
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("kael-notification-actions".into())
            .spawn(
                move || match prepare_action_listener(&title, &body, &actions) {
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
    actions: &[NotificationAction],
) -> Result<u32> {
    let actions = actions
        .iter()
        .flat_map(|action| [action.identifier.as_str(), action.title.as_str()])
        .collect::<Vec<_>>();
    let hints = HashMap::<&str, OwnedValue>::new();
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
    actions: &[NotificationAction],
) -> Result<(Proxy<'static>, SignalIterator<'static>, u32)> {
    let connection = Connection::session().context("failed to connect to the D-Bus session")?;
    let proxy = owned_notification_proxy(connection)?;
    let signals = proxy
        .receive_signal("ActionInvoked")
        .context("failed to subscribe to notification actions")?;
    let notification_id = send_notification(&proxy, title, body, actions)?;
    Ok((proxy, signals, notification_id))
}

fn forward_action(
    signals: &mut SignalIterator<'_>,
    notification_id: u32,
    on_action: Arc<dyn Fn(String) + Send + Sync + 'static>,
) -> Result<()> {
    for message in signals {
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
    Ok(())
}

fn body(notification: &LocalNotification) -> String {
    match notification.subtitle.as_deref() {
        Some(subtitle) if !subtitle.is_empty() => format!("{subtitle}\n{}", notification.body),
        _ => notification.body.clone(),
    }
}
