//! Validated desktop notification services for Kael applications.
//!
//! ```no_run
//! use kael_notifications::{LocalNotification, NotificationCenter};
//!
//! let center = NotificationCenter::new();
//! let id = center.schedule_local(LocalNotification::new(
//!     "Export complete",
//!     "report.pdf is ready",
//! ))?;
//! center.cancel(&id);
//! # Ok::<(), anyhow::Error>(())
//! ```

#![deny(missing_docs)]

/// Notification actions and categories.
pub mod action;
/// Typed portable failures and permission states.
pub mod error;
/// Local notification scheduling and event delivery.
pub mod local;
/// Platform metadata and delivery backends.
pub mod platform;
/// Push-token types.
pub mod push;

pub use action::{ActionOptions, NotificationAction, NotificationCategory};
pub use anyhow::Result;
pub use error::{NotificationError, NotificationOperationResult, NotificationPermissionStatus};
pub use local::{
    AuthorizationOptions, CircularRegion, DEFAULT_NOTIFICATION_ACTION_ID, DateComponents,
    LocalNotification, NotificationAttachment, NotificationCenter, NotificationEvent,
    NotificationId, NotificationPayload, NotificationSound, NotificationTrigger, Subscription,
};
pub use platform::PlatformNotificationSupport;
pub use push::PushToken;
