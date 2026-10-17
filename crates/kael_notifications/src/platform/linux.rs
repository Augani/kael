use std::{collections::HashMap, sync::Arc};

use anyhow::{Context as _, Result};
use zbus::{
    blocking::{Connection, Proxy, proxy::SignalIterator},
    zvariant::Value,
};

use crate::{
    action::NotificationAction,
    local::{DEFAULT_NOTIFICATION_ACTION_ID, LocalNotification, NotificationPayload},
};

use super::{NotificationBackend, PlatformNotificationSupport};

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "freedesktop-notifications-dbus",
    action_backend: "freedesktop-action-invoked",
    push_backend: "not-implemented",
    asynchronous_permission: false,
    immediate_delivery: true,
    interval_triggers: true,
    system_triggers: false,
    actions: true,
    cancellation: false,
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
        if on_action.is_none() {
            let connection =
                Connection::session().context("failed to connect to the D-Bus session")?;
            let proxy = notification_proxy(&connection)?;
            send_notification(
                &proxy,
                &payload.title,
                &body,
                &notification.sound,
                actions,
                false,
            )?;
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
                    Ok((_proxy, mut signals, notification_id, action_ids)) => {
                        let _ = ready_tx.send(Ok(()));
                        if let Err(error) =
                            forward_action(&mut signals, notification_id, &action_ids, on_action)
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

type ActionIds = HashMap<String, String>;

fn platform_actions(
    actions: &[NotificationAction],
    include_default_action: bool,
) -> (Vec<String>, ActionIds) {
    let mut wire = Vec::with_capacity(actions.len() * 2 + 2);
    let mut action_ids = HashMap::with_capacity(actions.len() + 1);
    if include_default_action {
        // The desktop protocol reserves this wire key for body activation.
        wire.extend(["default".into(), "Open".into()]);
        action_ids.insert("default".into(), DEFAULT_NOTIFICATION_ACTION_ID.into());
    }
    for (index, action) in actions.iter().enumerate() {
        // Authored IDs may themselves be default, __default, or __closed.
        // A private exact key keeps those valid public IDs independent of the
        // protocol's default action and avoids sentinel-name collisions.
        let key = format!("kael-action-{index}");
        wire.extend([key.clone(), action.title.clone()]);
        action_ids.insert(key, action.identifier.clone());
    }
    (wire, action_ids)
}

fn send_notification(
    proxy: &Proxy<'_>,
    title: &str,
    body: &str,
    sound: &Option<crate::local::NotificationSound>,
    actions: &[NotificationAction],
    include_default_action: bool,
) -> Result<(u32, ActionIds)> {
    let (platform_actions, action_ids) = platform_actions(actions, include_default_action);
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
    let notification_id = proxy
        .call(
            "Notify",
            &(
                "Kael",
                0_u32,
                "",
                title,
                body,
                platform_actions,
                hints,
                -1_i32,
            ),
        )
        .context("failed to show notification")?;
    Ok((notification_id, action_ids))
}

fn prepare_action_listener(
    title: &str,
    body: &str,
    sound: &Option<crate::local::NotificationSound>,
    actions: &[NotificationAction],
) -> Result<(Proxy<'static>, SignalIterator<'static>, u32, ActionIds)> {
    let connection = Connection::session().context("failed to connect to the D-Bus session")?;
    let proxy = owned_notification_proxy(connection)?;
    let signals = proxy
        .receive_all_signals()
        .context("failed to subscribe to notification lifecycle events")?;
    let (notification_id, action_ids) =
        send_notification(&proxy, title, body, sound, actions, true)?;
    Ok((proxy, signals, notification_id, action_ids))
}

#[derive(Debug, PartialEq, Eq)]
enum NotificationSignal {
    Ignore,
    Action(String),
    Closed,
}

fn notification_signal(
    message: &zbus::Message,
    notification_id: u32,
    action_ids: &ActionIds,
) -> Result<NotificationSignal> {
    match message.header().member().map(|member| member.as_str()) {
        Some("ActionInvoked") => {
            let (id, wire_id): (u32, String) = message
                .body()
                .deserialize()
                .context("invalid notification action signal")?;
            if id == notification_id
                && let Some(action_id) = action_ids.get(&wire_id)
            {
                return Ok(NotificationSignal::Action(action_id.clone()));
            }
        }
        Some("NotificationClosed") => {
            let (id, _reason): (u32, u32) = message
                .body()
                .deserialize()
                .context("invalid notification close signal")?;
            if id == notification_id {
                return Ok(NotificationSignal::Closed);
            }
        }
        _ => {}
    }
    Ok(NotificationSignal::Ignore)
}

fn forward_action(
    signals: impl Iterator<Item = zbus::Message>,
    notification_id: u32,
    action_ids: &ActionIds,
    on_action: Arc<dyn Fn(String) + Send + Sync + 'static>,
) -> Result<()> {
    for message in signals {
        match notification_signal(&message, notification_id, action_ids)? {
            NotificationSignal::Action(action_id) => {
                on_action(action_id);
                return Ok(());
            }
            NotificationSignal::Closed => return Ok(()),
            NotificationSignal::Ignore => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActionOptions;

    fn actions() -> [NotificationAction; 3] {
        ["default", "__closed", "__default"].map(|identifier| NotificationAction {
            identifier: identifier.into(),
            title: format!("Action {identifier}"),
            options: ActionOptions::default(),
            text_input_placeholder: None,
        })
    }

    fn action(id: u32, key: &str) -> zbus::Message {
        zbus::Message::signal(
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
        )
        .unwrap()
        .build(&(id, key))
        .unwrap()
    }

    fn closed(id: u32) -> zbus::Message {
        zbus::Message::signal(
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationClosed",
        )
        .unwrap()
        .build(&(id, 2_u32))
        .unwrap()
    }

    #[test]
    fn default_and_custom_action_keys_are_distinct_and_preserve_public_ids() {
        let (wire, ids) = platform_actions(&actions(), true);
        assert_eq!(
            wire,
            [
                "default",
                "Open",
                "kael-action-0",
                "Action default",
                "kael-action-1",
                "Action __closed",
                "kael-action-2",
                "Action __default",
            ]
        );
        assert_eq!(ids.len(), 4);
        assert_eq!(ids["default"], DEFAULT_NOTIFICATION_ACTION_ID);
        for (index, action) in actions().iter().enumerate() {
            assert_eq!(ids[&format!("kael-action-{index}")], action.identifier);
        }
        let (wire, ids) = platform_actions(&actions(), false);
        assert_eq!(wire.len(), 6);
        assert!(!ids.contains_key("default"));
        let (wire, ids) = platform_actions(&[], false);
        assert!(wire.is_empty() && ids.is_empty());
    }

    #[test]
    fn actual_wire_messages_map_actions_and_reject_malformed_lifecycle_bodies() {
        let (_, ids) = platform_actions(&actions(), true);
        for (key, public) in [
            ("default", DEFAULT_NOTIFICATION_ACTION_ID),
            ("kael-action-0", "default"),
            ("kael-action-1", "__closed"),
            ("kael-action-2", "__default"),
        ] {
            assert_eq!(
                notification_signal(&action(41, key), 41, &ids).unwrap(),
                NotificationSignal::Action(public.into())
            );
            assert_eq!(
                notification_signal(&action(42, key), 41, &ids).unwrap(),
                NotificationSignal::Ignore
            );
        }
        for unknown in ["__default", "__closed", "kael-action-00", "kael-action-3"] {
            assert_eq!(
                notification_signal(&action(41, unknown), 41, &ids).unwrap(),
                NotificationSignal::Ignore
            );
        }
        assert_eq!(
            notification_signal(&closed(41), 41, &ids).unwrap(),
            NotificationSignal::Closed
        );
        assert_eq!(
            notification_signal(&closed(42), 41, &ids).unwrap(),
            NotificationSignal::Ignore
        );
        for member in ["ActionInvoked", "NotificationClosed"] {
            let malformed = zbus::Message::signal(
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                member,
            )
            .unwrap()
            .build(&("not uint32", "not uint32"))
            .unwrap();
            assert!(notification_signal(&malformed, 41, &ids).is_err());
        }
    }

    #[test]
    fn forwarding_is_exactly_once_and_releases_action_capture_on_action_or_close() {
        let (_, ids) = platform_actions(&actions(), true);
        for (messages, expected) in [
            (
                vec![
                    action(42, "default"),
                    action(41, "__default"),
                    action(41, "kael-action-1"),
                    action(41, "kael-action-0"),
                ],
                vec!["__closed".to_string()],
            ),
            (
                vec![
                    closed(42),
                    action(41, "kael-action-00"),
                    closed(41),
                    action(41, "default"),
                ],
                Vec::new(),
            ),
        ] {
            let capture = Arc::new(());
            let weak = Arc::downgrade(&capture);
            let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
            let results = calls.clone();
            forward_action(
                messages.into_iter(),
                41,
                &ids,
                Arc::new(move |id| {
                    let _keep_alive = &capture;
                    results.lock().unwrap().push(id);
                }),
            )
            .unwrap();
            assert_eq!(*calls.lock().unwrap(), expected);
            assert!(weak.upgrade().is_none());
        }
    }
}
