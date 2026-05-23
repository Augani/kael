//! Local notification scheduling and event delivery.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    action::{NotificationAction, NotificationCategory},
    platform::{NotificationBackend, default_backend},
    push::PushToken,
};

type EventListener = Arc<dyn Fn(NotificationEvent) + Send + Sync + 'static>;

/// A handle that unregisters a notification callback when dropped.
#[must_use]
pub struct Subscription {
    unsubscribe: Option<Box<dyn FnOnce() + 'static>>,
}

impl Subscription {
    /// Creates a new subscription handle.
    pub fn new(unsubscribe: impl FnOnce() + 'static) -> Self {
        Self {
            unsubscribe: Some(Box::new(unsubscribe)),
        }
    }

    /// Detaches the callback from this handle.
    pub fn detach(mut self) {
        self.unsubscribe.take();
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
    }
}

/// The options requested during notification authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationOptions {
    /// Whether alert/banners are requested.
    pub alert: bool,
    /// Whether badge updates are requested.
    pub badge: bool,
    /// Whether notification sounds are requested.
    pub sound: bool,
}

impl Default for AuthorizationOptions {
    fn default() -> Self {
        Self {
            alert: true,
            badge: true,
            sound: true,
        }
    }
}

/// The sound policy for a notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationSound {
    /// Use the platform default sound.
    Default,
    /// Use a named sound if the platform supports it.
    Named(String),
    /// Deliver silently.
    Silent,
}

/// A file attachment associated with a local notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAttachment {
    /// The attached file path.
    pub path: PathBuf,
    /// The optional MIME type.
    pub mime_type: Option<String>,
}

/// Date components used by calendar-based notification triggers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DateComponents {
    /// The calendar year.
    pub year: Option<i32>,
    /// The calendar month.
    pub month: Option<u32>,
    /// The calendar day.
    pub day: Option<u32>,
    /// The hour component.
    pub hour: Option<u32>,
    /// The minute component.
    pub minute: Option<u32>,
    /// The second component.
    pub second: Option<u32>,
}

/// A circular geographic region used by location triggers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularRegion {
    /// The region identifier.
    pub identifier: String,
    /// The center latitude.
    pub latitude: f64,
    /// The center longitude.
    pub longitude: f64,
    /// The radius in meters.
    pub radius_meters: f64,
}

/// The trigger controlling when a notification fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotificationTrigger {
    /// Deliver immediately.
    Immediate,
    /// Deliver after a time interval.
    TimeInterval {
        /// The delay in seconds.
        seconds: f64,
        /// Whether the notification should repeat.
        repeats: bool,
    },
    /// Deliver on a calendar date.
    Calendar {
        /// The calendar components to match.
        date_components: DateComponents,
        /// Whether the notification should repeat.
        repeats: bool,
    },
    /// Deliver when entering or leaving a geographic region.
    Location {
        /// The target region.
        region: CircularRegion,
        /// Whether the trigger should fire on entry.
        on_entry: bool,
        /// Whether the trigger should fire on exit.
        on_exit: bool,
    },
}

/// A locally scheduled notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalNotification {
    /// The notification title.
    pub title: String,
    /// The notification body.
    pub body: String,
    /// The optional subtitle.
    pub subtitle: Option<String>,
    /// The notification sound.
    pub sound: Option<NotificationSound>,
    /// The badge count to apply when delivered.
    pub badge: Option<u32>,
    /// The category identifier used to resolve actions.
    pub category: Option<String>,
    /// Additional user info attached to the notification.
    pub user_info: HashMap<String, String>,
    /// The trigger describing when to deliver the notification.
    pub trigger: NotificationTrigger,
    /// Attached files associated with the notification.
    pub attachments: Vec<NotificationAttachment>,
}

/// A unique notification identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NotificationId(pub u64);

/// The delivered notification payload observed by listeners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationPayload {
    /// The notification identifier.
    pub id: NotificationId,
    /// The title shown to the user.
    pub title: String,
    /// The body shown to the user.
    pub body: String,
    /// The optional subtitle.
    pub subtitle: Option<String>,
    /// Arbitrary user info associated with the notification.
    pub user_info: HashMap<String, String>,
}

/// Notification events emitted by the notification center.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationEvent {
    /// A notification was delivered.
    Received(NotificationPayload),
    /// A user performed an action on a delivered notification.
    ActionPerformed {
        /// The delivered notification payload.
        notification: NotificationPayload,
        /// The selected action identifier.
        action_id: String,
        /// Optional text entered by the user.
        text_input: Option<String>,
    },
    /// A scheduled notification was dismissed before delivery.
    Dismissed(NotificationPayload),
}

/// A notification center that manages local scheduling and event delivery.
#[derive(Clone)]
pub struct NotificationCenter {
    inner: Arc<NotificationCenterState>,
}

struct NotificationCenterState {
    backend: Arc<dyn NotificationBackend>,
    next_notification_id: AtomicU64,
    next_listener_id: AtomicUsize,
    badge_count: AtomicU32,
    scheduled: Mutex<HashMap<NotificationId, ScheduledNotification>>,
    listeners: Mutex<BTreeMap<usize, EventListener>>,
    categories: Mutex<HashMap<String, NotificationCategory>>,
}

struct ScheduledNotification {
    cancelled: Arc<AtomicBool>,
}

impl NotificationCenter {
    /// Creates a new notification center using the platform backend.
    pub fn new() -> Self {
        Self::with_backend(default_backend())
    }

    /// Registers a notification category for future local notifications.
    pub fn register_category(&self, category: NotificationCategory) {
        self.inner
            .categories
            .lock()
            .insert(category.identifier.clone(), category);
    }

    /// Requests notification authorization from the platform backend.
    pub async fn request_authorization(&self, options: AuthorizationOptions) -> Result<bool> {
        self.inner.backend.request_authorization(&options)
    }

    /// Schedules a local notification and returns its identifier.
    pub fn schedule_local(&self, notification: LocalNotification) -> Result<NotificationId> {
        let actions = self.resolve_actions(notification.category.as_deref())?;
        let notification_id = NotificationId(
            self.inner
                .next_notification_id
                .fetch_add(1, Ordering::Relaxed),
        );
        let trigger = notification.trigger.clone();

        match trigger {
            NotificationTrigger::Immediate => {
                self.deliver_once(notification_id, notification, actions)?;
                Ok(notification_id)
            }
            NotificationTrigger::TimeInterval { seconds, repeats } => {
                if !seconds.is_finite() || seconds.is_sign_negative() {
                    return Err(anyhow!("notification time interval must be a finite positive value"));
                }
                if repeats && seconds == 0.0 {
                    return Err(anyhow!("repeating notification intervals must be greater than zero"));
                }

                let delay = Duration::from_secs_f64(seconds.max(0.0));
                let cancelled = Arc::new(AtomicBool::new(false));
                self.inner
                    .scheduled
                    .lock()
                    .insert(notification_id, ScheduledNotification { cancelled: cancelled.clone() });
                self.spawn_delivery_loop(notification_id, notification, actions, cancelled, delay, repeats);
                Ok(notification_id)
            }
            NotificationTrigger::Calendar { .. } => Err(anyhow!(
                "calendar notification triggers are not implemented in kael_notifications yet"
            )),
            NotificationTrigger::Location { .. } => Err(anyhow!(
                "location notification triggers are not implemented in kael_notifications yet"
            )),
        }
    }

    /// Cancels a previously scheduled notification.
    pub fn cancel(&self, id: &NotificationId) {
        if let Some(entry) = self.inner.scheduled.lock().remove(id) {
            entry.cancelled.store(true, Ordering::Relaxed);
            let payload = NotificationPayload {
                id: *id,
                title: String::new(),
                body: String::new(),
                subtitle: None,
                user_info: HashMap::new(),
            };
            self.emit(NotificationEvent::Dismissed(payload));
        }
    }

    /// Cancels all scheduled notifications.
    pub fn cancel_all(&self) {
        let entries = self
            .inner
            .scheduled
            .lock()
            .drain()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        for entry in entries {
            entry.cancelled.store(true, Ordering::Relaxed);
        }
    }

    /// Registers for push notifications when the backend supports it.
    pub async fn register_for_push(&self) -> Result<PushToken> {
        self.inner.backend.register_for_push()
    }

    /// Registers an event listener for delivered notifications and actions.
    pub fn on_received(
        &self,
        callback: impl Fn(NotificationEvent) + Send + Sync + 'static,
    ) -> Subscription {
        let state = self.inner.clone();
        let listener_id = self
            .inner
            .next_listener_id
            .fetch_add(1, Ordering::Relaxed);
        state.listeners.lock().insert(listener_id, Arc::new(callback));

        Subscription::new(move || {
            state.listeners.lock().remove(&listener_id);
        })
    }

    /// Sets the application badge count.
    pub fn set_badge_count(&self, count: u32) {
        self.inner.badge_count.store(count, Ordering::Relaxed);
        let _ = self.inner.backend.set_badge_count(count);
    }

    pub(crate) fn with_backend(backend: Arc<dyn NotificationBackend>) -> Self {
        Self {
            inner: Arc::new(NotificationCenterState {
                backend,
                next_notification_id: AtomicU64::new(1),
                next_listener_id: AtomicUsize::new(0),
                badge_count: AtomicU32::new(0),
                scheduled: Mutex::new(HashMap::new()),
                listeners: Mutex::new(BTreeMap::new()),
                categories: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn resolve_actions(&self, category: Option<&str>) -> Result<Vec<NotificationAction>> {
        let Some(category) = category else {
            return Ok(Vec::new());
        };
        self.inner
            .categories
            .lock()
            .get(category)
            .map(|category| category.actions.clone())
            .ok_or_else(|| anyhow!("notification category {category} is not registered"))
    }

    fn spawn_delivery_loop(
        &self,
        id: NotificationId,
        notification: LocalNotification,
        actions: Vec<NotificationAction>,
        cancelled: Arc<AtomicBool>,
        delay: Duration,
        repeats: bool,
    ) {
        let center = self.clone();
        std::thread::spawn(move || {
            loop {
                if delay > Duration::ZERO {
                    std::thread::sleep(delay);
                }
                if cancelled.load(Ordering::Relaxed) {
                    break;
                }
                if center
                    .deliver_once(id, notification.clone(), actions.clone())
                    .is_err()
                {
                    break;
                }
                if !repeats {
                    center.inner.scheduled.lock().remove(&id);
                    break;
                }
            }
        });
    }

    fn deliver_once(
        &self,
        id: NotificationId,
        notification: LocalNotification,
        actions: Vec<NotificationAction>,
    ) -> Result<()> {
        let payload = NotificationPayload::from_notification(id, &notification);
        let action_callback = if actions.is_empty() {
            None
        } else {
            let center = self.clone();
            let payload = payload.clone();
            Some(Arc::new(move |action_id: String| {
                center.emit(NotificationEvent::ActionPerformed {
                    notification: payload.clone(),
                    action_id,
                    text_input: None,
                });
            }) as Arc<dyn Fn(String) + Send + Sync + 'static>)
        };
        self.inner
            .backend
            .deliver(&payload, &notification, &actions, action_callback)?;
        if let Some(badge) = notification.badge {
            self.set_badge_count(badge);
        }
        self.emit(NotificationEvent::Received(payload));
        Ok(())
    }

    fn emit(&self, event: NotificationEvent) {
        let listeners = self
            .inner
            .listeners
            .lock()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for listener in listeners {
            listener(event.clone());
        }
    }
}

impl Default for NotificationCenter {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationPayload {
    fn from_notification(id: NotificationId, notification: &LocalNotification) -> Self {
        Self {
            id,
            title: notification.title.clone(),
            body: notification.body.clone(),
            subtitle: notification.subtitle.clone(),
            user_info: notification.user_info.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::{Duration, Instant}};

    use parking_lot::Mutex;

    use crate::{
        action::{ActionOptions, NotificationAction, NotificationCategory},
        platform::NotificationBackend,
        push::PushToken,
    };

    use super::{
        AuthorizationOptions, LocalNotification, NotificationCenter, NotificationEvent,
        NotificationPayload, NotificationTrigger,
    };

    #[derive(Default)]
    struct MockBackend {
        delivered: Mutex<Vec<NotificationPayload>>,
        action_to_emit: Mutex<Option<String>>,
    }

    impl NotificationBackend for MockBackend {
        fn request_authorization(&self, _options: &AuthorizationOptions) -> anyhow::Result<bool> {
            Ok(true)
        }

        fn deliver(
            &self,
            payload: &NotificationPayload,
            _notification: &LocalNotification,
            _actions: &[NotificationAction],
            on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
        ) -> anyhow::Result<()> {
            self.delivered.lock().push(payload.clone());
            if let Some(action_id) = self.action_to_emit.lock().clone() {
                if let Some(on_action) = on_action {
                    on_action(action_id);
                }
            }
            Ok(())
        }

        fn register_for_push(&self) -> anyhow::Result<PushToken> {
            Ok(PushToken(vec![1, 2, 3]))
        }
    }

    #[test]
    fn immediate_notifications_emit_received_events() {
        let backend = Arc::new(MockBackend::default());
        let center = NotificationCenter::with_backend(backend.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed_events = events.clone();
        let _subscription = center.on_received(move |event| {
            observed_events.lock().push(event);
        });

        let notification = LocalNotification {
            title: "Hello".into(),
            body: "World".into(),
            subtitle: None,
            sound: None,
            badge: Some(2),
            category: None,
            user_info: Default::default(),
            trigger: NotificationTrigger::Immediate,
            attachments: Vec::new(),
        };
        center.schedule_local(notification).unwrap();

        assert_eq!(backend.delivered.lock().len(), 1);
        assert!(events
            .lock()
            .iter()
            .any(|event| matches!(event, NotificationEvent::Received(_))));
    }

    #[test]
    fn cancelling_delayed_notifications_prevents_delivery() {
        let backend = Arc::new(MockBackend::default());
        let center = NotificationCenter::with_backend(backend.clone());
        let notification = LocalNotification {
            title: "Later".into(),
            body: "Soon".into(),
            subtitle: None,
            sound: None,
            badge: None,
            category: None,
            user_info: Default::default(),
            trigger: NotificationTrigger::TimeInterval {
                seconds: 0.05,
                repeats: false,
            },
            attachments: Vec::new(),
        };

        let id = center.schedule_local(notification).unwrap();
        center.cancel(&id);
        std::thread::sleep(Duration::from_millis(100));
        assert!(backend.delivered.lock().is_empty());
    }

    #[test]
    fn action_callbacks_emit_action_events() {
        let backend = Arc::new(MockBackend::default());
        *backend.action_to_emit.lock() = Some("open".to_string());
        let center = NotificationCenter::with_backend(backend);
        center.register_category(NotificationCategory {
            identifier: "message".into(),
            actions: vec![NotificationAction {
                identifier: "open".into(),
                title: "Open".into(),
                options: ActionOptions {
                    foreground: true,
                    destructive: false,
                    authentication_required: false,
                },
                text_input_placeholder: None,
            }],
        });
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed_events = events.clone();
        let _subscription = center.on_received(move |event| {
            observed_events.lock().push(event);
        });

        center
            .schedule_local(LocalNotification {
                title: "Action".into(),
                body: "Required".into(),
                subtitle: None,
                sound: None,
                badge: None,
                category: Some("message".into()),
                user_info: Default::default(),
                trigger: NotificationTrigger::Immediate,
                attachments: Vec::new(),
            })
            .unwrap();

        wait_until(Duration::from_millis(100), || {
            events.lock().iter().any(|event| matches!(
                event,
                NotificationEvent::ActionPerformed { action_id, .. } if action_id == "open"
            ))
        });
        assert!(events.lock().iter().any(|event| matches!(
            event,
            NotificationEvent::Received(payload) if payload.title == "Action"
        )));
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if predicate() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("condition was not satisfied before timeout");
    }
}