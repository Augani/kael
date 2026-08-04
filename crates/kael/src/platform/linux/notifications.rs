use std::collections::HashMap;

use anyhow::{Context as _, Result};
use zbus::{
    blocking::{Connection, Proxy, proxy::SignalIterator},
    zvariant::OwnedValue,
};

use crate::NotificationAction;

pub fn show_notification(title: &str, body: &str) -> Result<()> {
    let connection = Connection::session().context("failed to connect to the D-Bus session")?;
    let proxy = notification_proxy(&connection)?;
    send_notification(&proxy, title, body, &[])?;
    Ok(())
}

/// Show a notification with action buttons using the `org.freedesktop.Notifications`
/// D-Bus `Notify` method's `actions` parameter.
///
/// The `callback` is invoked with the action `id` when the user clicks an action button.
/// A background thread listens for the freedesktop `ActionInvoked` signal and uses a
/// oneshot channel to deliver the action ID back to the foreground executor.
pub fn show_notification_with_actions(
    title: &str,
    body: &str,
    actions: &[NotificationAction],
    mut callback: Box<dyn FnMut(String)>,
    foreground_executor: crate::ForegroundExecutor,
) -> Result<()> {
    let (tx, rx) = futures::channel::oneshot::channel::<String>();
    let title = title.to_owned();
    let body = body.to_owned();
    let actions = actions.to_vec();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);

    std::thread::Builder::new()
        .name("kael-notification-actions".into())
        .spawn(
            move || match prepare_action_listener(&title, &body, &actions) {
                Ok((_proxy, mut signals, notification_id)) => {
                    let _ = ready_tx.send(Ok(()));
                    if let Err(error) = forward_action(&mut signals, notification_id, tx) {
                        log::error!("notification action listener failed: {error:#}");
                    }
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    let _ = ready_tx.send(Err(message));
                }
            },
        )
        .context("failed to start the notification action listener")?;

    let delivery = ready_rx
        .recv()
        .context("notification action listener stopped before delivery")?;
    delivery.map_err(anyhow::Error::msg)?;

    foreground_executor
        .spawn(async move {
            if let Ok(action_id) = rx.await {
                super::catch_platform_callback("notification action", (), || callback(action_id));
            }
        })
        .detach();
    Ok(())
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
        .flat_map(|action| [action.id.as_str(), action.label.as_str()])
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
    tx: futures::channel::oneshot::Sender<String>,
) -> Result<()> {
    for message in signals {
        let (id, action_id): (u32, String) = message
            .body()
            .deserialize()
            .context("invalid notification action signal")?;
        if id == notification_id {
            let _ = tx.send(action_id);
            return Ok(());
        }
    }
    Ok(())
}
