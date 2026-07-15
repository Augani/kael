use anyhow::Result;

use crate::NotificationAction;

pub fn show_notification(title: &str, body: &str) -> Result<()> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show()
        .map_err(|e| anyhow::anyhow!("Failed to show notification: {}", e))?;
    Ok(())
}

/// Show a notification with action buttons using the `org.freedesktop.Notifications`
/// D-Bus `Notify` method's `actions` parameter.
///
/// The `callback` is invoked with the action `id` when the user clicks an action button.
/// Because `notify-rust`'s `wait_for_action` blocks, we spawn a background thread to
/// listen for the user's response and use a oneshot channel to deliver the action ID
/// back to a foreground task where the callback is invoked.
pub fn show_notification_with_actions(
    title: &str,
    body: &str,
    actions: &[NotificationAction],
    mut callback: Box<dyn FnMut(String)>,
    foreground_executor: crate::ForegroundExecutor,
) -> Result<()> {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(body);

    for action in actions {
        notification.action(&action.id, &action.label);
    }

    let handle = notification
        .show()
        .map_err(|e| anyhow::anyhow!("Failed to show notification: {}", e))?;

    // Set up a oneshot channel: the sender goes to the background thread,
    // the receiver stays on the foreground where the callback lives.
    let (tx, rx) = futures::channel::oneshot::channel::<String>();

    // Spawn a foreground task that awaits the action ID and invokes the callback.
    // This keeps the non-Send callback on the main thread.
    foreground_executor
        .spawn(async move {
            if let Ok(action_id) = rx.await {
                super::catch_platform_callback("notification action", (), || callback(action_id));
            }
        })
        .detach();

    // Spawn a background thread that blocks on `wait_for_action` and sends
    // the action ID through the channel when the user clicks a button.
    std::thread::spawn(move || {
        let tx = std::sync::Mutex::new(Some(tx));
        handle.wait_for_action(|action_id| {
            // "__closed" is sent when the notification is dismissed without
            // clicking an action button — skip it.
            if action_id == "__closed" {
                return;
            }
            if let Some(sender) = tx.lock().ok().and_then(|mut guard| guard.take()) {
                let _ = sender.send(action_id.to_string());
            }
        });
    });

    Ok(())
}
