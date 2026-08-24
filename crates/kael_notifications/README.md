# kael_notifications

Validated local notification scheduling and platform delivery for
[Kael](https://github.com/Augani/kael).

The crate provides immediate notifications, native time-interval scheduling,
categories and actions, cancellation, event subscriptions, bounded validation,
sound policies, and delivery on macOS, Windows, Linux, and browsers.

Portable applications should use the asynchronous call on every target:

```rust,no_run
# async fn example() -> kael_notifications::NotificationOperationResult<()> {
use kael_notifications::{LocalNotification, NotificationCenter};

let center = NotificationCenter::new();
center
    .schedule_local_async(LocalNotification::new("Export complete", "report.pdf is ready"))
    .await?;
# Ok(())
# }
```

In a browser, this call requests permission when the state is still `Prompt`.
It must therefore start from a click or keyboard activation. `schedule_local`
remains available for existing desktop code and for browser calls after
permission is already granted; it returns the typed
`PermissionPromptRequired`/`UserActivationRequired`/`PermissionDenied` error
otherwise.

Platform setup still belongs to the application:

- Windows installers must register an AppUserModelID; call
  `set_windows_app_user_model_id` before toast delivery.
- Linux delivery depends on the user's D-Bus notification service.
- Browser delivery uses the Notification API, retains at most 256 live handles,
  and supports notification-body clicks and explicit close-by-ID cancellation.
  Page-created notifications do not provide durable interval/calendar/location
  scheduling, custom action buttons, named sounds, push, or badge counts. These
  requests return typed errors; Kael does not substitute a hidden page timer or
  pretend a service worker exists.
- Push registration needs product credentials and delegates and is not provided.
- Calendar/location triggers, attachments, text-input actions, and application
  badge counts are not implemented in Kael 0.4. Requests for these features
  return errors instead of being silently ignored.

Query `platform::support()` when behavior depends on a specific backend.

## License

Apache-2.0. See `LICENSE-APACHE`.
