# kael_notifications

Validated local notification scheduling and platform delivery for
[Kael](https://github.com/Augani/kael).

The crate provides immediate and time-interval notifications, categories and
actions, cancellation, event subscriptions, bounded validation, native sound
policies, and delivery on macOS, Windows, and Linux.

Platform setup still belongs to the application:

- Windows installers must register an AppUserModelID; call
  `set_windows_app_user_model_id` before toast delivery.
- Linux delivery depends on the user's D-Bus notification service.
- Push registration needs product credentials and delegates and is not provided.
- Calendar/location triggers, attachments, text-input actions, and application
  badge counts are not implemented in Kael 0.3. Requests for these features
  return errors instead of being silently ignored.

Query `platform::support()` when behavior depends on a specific backend.

## License

Apache-2.0. See `LICENSE-APACHE`.
