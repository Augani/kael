# kael_notifications

Validated local notification scheduling and platform delivery for
[Kael](https://github.com/Augani/kael).

The crate provides immediate and time-interval notifications, categories and
actions, cancellation, event subscriptions, badge state, bounded validation,
and native delivery on macOS, Windows, and Linux.

Platform setup still belongs to the application:

- Windows installers must register an AppUserModelID; call
  `set_windows_app_user_model_id` before toast delivery.
- Linux delivery depends on the user's D-Bus notification service.
- Push registration needs product credentials and delegates and is not provided.
- Calendar/location triggers and macOS text-input actions are not implemented in
  Kael 0.3.

Query `platform::support()` when behavior depends on a specific backend.

## License

Apache-2.0. See `LICENSE-APACHE`.
