//! Notification services for Kael.

#![deny(missing_docs)]

/// Notification actions and categories.
pub mod action;
/// Local notification scheduling and event delivery.
pub mod local;
/// Platform metadata and delivery backends.
pub mod platform;
/// Push-token types.
pub mod push;

pub use anyhow::Result;
pub use action::{ActionOptions, NotificationAction, NotificationCategory};
pub use local::{
    AuthorizationOptions, CircularRegion, DateComponents, LocalNotification, NotificationAttachment,
    NotificationCenter, NotificationEvent, NotificationId, NotificationPayload, NotificationSound,
    NotificationTrigger, Subscription,
};
pub use push::PushToken;