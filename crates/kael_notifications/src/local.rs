//! Local notification scheduling and event delivery.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use parking_lot::{Condvar, Mutex};
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
    /// Use a platform-native named sound if the backend supports it.
    Named(String),
    /// Deliver silently.
    Silent,
}

/// A file attachment reserved for backends that support rich notifications.
///
/// The bundled backends currently reject notification attachments.
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
    /// The application badge count to apply when delivered.
    ///
    /// The bundled backends currently reject badge updates.
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

impl LocalNotification {
    /// Creates an immediate notification with the required visible content.
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            subtitle: None,
            sound: None,
            badge: None,
            category: None,
            user_info: HashMap::new(),
            trigger: NotificationTrigger::Immediate,
            attachments: Vec::new(),
        }
    }
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

struct SchedulerEntry {
    wake_at: Instant,
    id: NotificationId,
    notification: LocalNotification,
    actions: Vec<NotificationAction>,
    cancelled: Arc<AtomicBool>,
    repeats: bool,
    delay: Duration,
}

impl PartialEq for SchedulerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.wake_at == other.wake_at
    }
}

impl Eq for SchedulerEntry {}

impl PartialOrd for SchedulerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SchedulerEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Reverse(self.wake_at).cmp(&Reverse(other.wake_at))
    }
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
    scheduled: Mutex<HashMap<NotificationId, ScheduledNotification>>,
    listeners: Mutex<BTreeMap<usize, EventListener>>,
    categories: Mutex<HashMap<String, NotificationCategory>>,
    scheduler_queue: Arc<(Mutex<BinaryHeap<SchedulerEntry>>, Condvar)>,
    scheduler_thread: Mutex<Option<JoinHandle<()>>>,
    scheduler_shutdown: Arc<AtomicBool>,
}

struct ScheduledNotification {
    cancelled: Arc<AtomicBool>,
    payload: NotificationPayload,
}

impl NotificationCenter {
    /// Creates a new notification center using the platform backend.
    pub fn new() -> Self {
        Self::with_backend(default_backend())
    }

    /// Registers a notification category for future local notifications.
    pub fn register_category(&self, category: NotificationCategory) -> Result<()> {
        validate_category(&category)?;
        self.inner
            .categories
            .lock()
            .insert(category.identifier.clone(), category);
        Ok(())
    }

    /// Requests notification authorization from the platform backend.
    pub async fn request_authorization(&self, options: AuthorizationOptions) -> Result<bool> {
        self.inner.backend.request_authorization(&options)
    }

    /// Schedules a local notification and returns its identifier.
    pub fn schedule_local(&self, notification: LocalNotification) -> Result<NotificationId> {
        validate_notification(&notification)?;
        let actions = self.resolve_actions(notification.category.as_deref())?;
        let notification_id = NotificationId(
            self.inner
                .next_notification_id
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| anyhow!("notification identifiers are exhausted"))?,
        );
        let trigger = notification.trigger.clone();

        match trigger {
            NotificationTrigger::Immediate => {
                self.deliver_once(notification_id, notification, actions)?;
                Ok(notification_id)
            }
            NotificationTrigger::TimeInterval { seconds, repeats } => {
                if repeats && seconds == 0.0 {
                    return Err(anyhow!(
                        "repeating notification intervals must be greater than zero"
                    ));
                }

                let delay = Duration::try_from_secs_f64(seconds)
                    .map_err(|_| anyhow!("notification time interval is out of range"))?;
                let _ = Instant::now()
                    .checked_add(delay)
                    .ok_or_else(|| anyhow!("notification time interval is too large"))?;
                let cancelled = Arc::new(AtomicBool::new(false));
                let payload =
                    NotificationPayload::from_notification(notification_id, &notification);
                let mut scheduled = self.inner.scheduled.lock();
                anyhow::ensure!(
                    scheduled.len() < 100_000,
                    "notification schedule contains too many pending entries"
                );
                scheduled.insert(
                    notification_id,
                    ScheduledNotification {
                        cancelled: cancelled.clone(),
                        payload,
                    },
                );
                drop(scheduled);
                self.schedule_delivery(
                    notification_id,
                    notification,
                    actions,
                    cancelled,
                    delay,
                    repeats,
                );
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
            let (queue, cvar) = &*self.inner.scheduler_queue;
            queue.lock().retain(|candidate| candidate.id != *id);
            cvar.notify_one();
            self.emit(NotificationEvent::Dismissed(entry.payload));
        }
    }

    /// Cancels all scheduled notifications and emits a dismissal for each one.
    pub fn cancel_all(&self) {
        let entries = self
            .inner
            .scheduled
            .lock()
            .drain()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        for entry in &entries {
            entry.cancelled.store(true, Ordering::Relaxed);
        }
        let (queue, cvar) = &*self.inner.scheduler_queue;
        queue.lock().clear();
        cvar.notify_one();
        for entry in entries {
            self.emit(NotificationEvent::Dismissed(entry.payload));
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
        let callback: EventListener = Arc::new(callback);
        let listener_id = loop {
            let candidate = self.inner.next_listener_id.fetch_add(1, Ordering::Relaxed);
            let mut listeners = state.listeners.lock();
            if let std::collections::btree_map::Entry::Vacant(entry) = listeners.entry(candidate) {
                entry.insert(callback.clone());
                break candidate;
            }
        };

        Subscription::new(move || {
            state.listeners.lock().remove(&listener_id);
        })
    }

    /// Sets the application badge count when the active backend supports it.
    ///
    /// The bundled Kael backends currently return an explicit unsupported error.
    pub fn set_badge_count(&self, count: u32) -> Result<()> {
        self.inner.backend.set_badge_count(count)
    }

    pub(crate) fn with_backend(backend: Arc<dyn NotificationBackend>) -> Self {
        let scheduler_queue: Arc<(Mutex<BinaryHeap<SchedulerEntry>>, Condvar)> =
            Arc::new((Mutex::new(BinaryHeap::new()), Condvar::new()));

        let queue_ref = scheduler_queue;
        let scheduler_shutdown = Arc::new(AtomicBool::new(false));

        let state = Arc::new_cyclic(|weak: &std::sync::Weak<NotificationCenterState>| {
            let weak_inner = weak.clone();
            let thread_queue = queue_ref.clone();
            let thread_shutdown = scheduler_shutdown.clone();

            let handle = std::thread::spawn(move || {
                let (lock, cvar) = &*thread_queue;
                loop {
                    let mut queue = lock.lock();
                    loop {
                        if thread_shutdown.load(Ordering::Acquire) {
                            return;
                        }
                        let now = Instant::now();
                        if queue.is_empty() {
                            cvar.wait(&mut queue);
                        } else if queue.peek().is_some_and(|e| e.wake_at > now) {
                            let delta = queue.peek().unwrap().wake_at - now;
                            cvar.wait_for(&mut queue, delta);
                        } else {
                            break;
                        }
                    }

                    let now = Instant::now();
                    let mut due = Vec::new();
                    while queue.peek().is_some_and(|e| e.wake_at <= now) {
                        if let Some(entry) = queue.pop() {
                            due.push(entry);
                        }
                    }
                    drop(queue);

                    let Some(inner) = weak_inner.upgrade() else {
                        break;
                    };
                    let center = NotificationCenter { inner };

                    for entry in due {
                        if entry.cancelled.load(Ordering::Relaxed) {
                            continue;
                        }
                        if center
                            .deliver_once(
                                entry.id,
                                entry.notification.clone(),
                                entry.actions.clone(),
                            )
                            .is_err()
                        {
                            center.inner.scheduled.lock().remove(&entry.id);
                            continue;
                        }
                        if entry.repeats {
                            let Some(wake_at) = Instant::now().checked_add(entry.delay) else {
                                center.inner.scheduled.lock().remove(&entry.id);
                                continue;
                            };
                            let next_entry = SchedulerEntry {
                                wake_at,
                                id: entry.id,
                                notification: entry.notification,
                                actions: entry.actions,
                                cancelled: entry.cancelled,
                                repeats: entry.repeats,
                                delay: entry.delay,
                            };
                            let (lock, cvar) = &*thread_queue;
                            lock.lock().push(next_entry);
                            cvar.notify_one();
                        } else {
                            center.inner.scheduled.lock().remove(&entry.id);
                        }
                    }
                }
            });

            NotificationCenterState {
                backend,
                next_notification_id: AtomicU64::new(1),
                next_listener_id: AtomicUsize::new(0),
                scheduled: Mutex::new(HashMap::new()),
                listeners: Mutex::new(BTreeMap::new()),
                categories: Mutex::new(HashMap::new()),
                scheduler_queue: queue_ref,
                scheduler_thread: Mutex::new(Some(handle)),
                scheduler_shutdown,
            }
        });

        Self { inner: state }
    }

    /// Returns the number of background scheduler threads (always 1).
    pub fn scheduler_thread_count(&self) -> usize {
        1
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

    fn schedule_delivery(
        &self,
        id: NotificationId,
        notification: LocalNotification,
        actions: Vec<NotificationAction>,
        cancelled: Arc<AtomicBool>,
        delay: Duration,
        repeats: bool,
    ) {
        let now = Instant::now();
        let wake_at = now.checked_add(delay).unwrap_or(now);
        let entry = SchedulerEntry {
            wake_at,
            id,
            notification,
            actions,
            cancelled,
            repeats,
            delay,
        };
        let (lock, cvar) = &*self.inner.scheduler_queue;
        lock.lock().push(entry);
        cvar.notify_one();
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
            })
                as Arc<dyn Fn(String) + Send + Sync + 'static>)
        };
        self.inner
            .backend
            .deliver(&payload, &notification, &actions, action_callback)?;
        if let Some(badge) = notification.badge {
            self.set_badge_count(badge)?;
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
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener(event.clone());
            }));
        }
    }
}

impl Drop for NotificationCenterState {
    fn drop(&mut self) {
        self.scheduler_shutdown.store(true, Ordering::Release);
        self.scheduler_queue.1.notify_all();
        self.scheduler_thread.get_mut().take();
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

fn validate_notification(notification: &LocalNotification) -> Result<()> {
    const MAX_TITLE_BYTES: usize = 512;
    const MAX_BODY_BYTES: usize = 64 * 1024;
    const MAX_USER_INFO_ENTRIES: usize = 256;

    anyhow::ensure!(
        !notification.title.trim().is_empty() && notification.title.len() <= MAX_TITLE_BYTES,
        "notification title must be non-empty and at most {MAX_TITLE_BYTES} bytes"
    );
    anyhow::ensure!(
        notification.body.len() <= MAX_BODY_BYTES,
        "notification body exceeds the {MAX_BODY_BYTES} byte limit"
    );
    if let Some(subtitle) = &notification.subtitle {
        anyhow::ensure!(
            subtitle.len() <= MAX_TITLE_BYTES,
            "notification subtitle exceeds the {MAX_TITLE_BYTES} byte limit"
        );
    }
    if let Some(NotificationSound::Named(name)) = &notification.sound {
        validate_identifier(name, "notification sound")?;
    }
    if let Some(category) = &notification.category {
        validate_identifier(category, "notification category")?;
    }
    anyhow::ensure!(
        notification.user_info.len() <= MAX_USER_INFO_ENTRIES,
        "notification user info contains too many entries"
    );
    for (key, value) in &notification.user_info {
        anyhow::ensure!(
            !key.is_empty() && key.len() <= 1_024 && value.len() <= MAX_BODY_BYTES,
            "notification user info contains an invalid key or oversized value"
        );
    }
    anyhow::ensure!(
        notification.attachments.is_empty(),
        "notification attachments are not implemented in kael_notifications yet"
    );
    match &notification.trigger {
        NotificationTrigger::Immediate => {}
        NotificationTrigger::TimeInterval { seconds, repeats } => {
            anyhow::ensure!(
                seconds.is_finite() && *seconds >= 0.0,
                "notification time interval must be finite and non-negative"
            );
            anyhow::ensure!(
                !repeats || *seconds > 0.0,
                "repeating notification intervals must be greater than zero"
            );
        }
        NotificationTrigger::Calendar {
            date_components, ..
        } => {
            validate_date_components(date_components)?;
        }
        NotificationTrigger::Location {
            region,
            on_entry,
            on_exit,
        } => {
            validate_identifier(&region.identifier, "notification region")?;
            anyhow::ensure!(
                region.latitude.is_finite()
                    && (-90.0..=90.0).contains(&region.latitude)
                    && region.longitude.is_finite()
                    && (-180.0..=180.0).contains(&region.longitude)
                    && region.radius_meters.is_finite()
                    && region.radius_meters > 0.0,
                "notification region has invalid coordinates or radius"
            );
            anyhow::ensure!(
                *on_entry || *on_exit,
                "location trigger must fire on entry or exit"
            );
        }
    }
    Ok(())
}

fn validate_category(category: &NotificationCategory) -> Result<()> {
    validate_identifier(&category.identifier, "notification category")?;
    anyhow::ensure!(
        category.actions.len() <= 16,
        "notification category has too many actions"
    );
    let mut identifiers = std::collections::HashSet::new();
    for action in &category.actions {
        validate_identifier(&action.identifier, "notification action")?;
        anyhow::ensure!(
            identifiers.insert(action.identifier.as_str()),
            "notification category contains duplicate action identifiers"
        );
        anyhow::ensure!(
            !action.title.trim().is_empty() && action.title.len() <= 256,
            "notification action title is invalid"
        );
        if let Some(placeholder) = &action.text_input_placeholder {
            anyhow::ensure!(
                placeholder.len() <= 256,
                "notification text-input placeholder is too large"
            );
            return Err(anyhow!(
                "text-input notification actions are not implemented in kael_notifications yet"
            ));
        }
    }
    Ok(())
}

fn validate_identifier(identifier: &str, label: &str) -> Result<()> {
    anyhow::ensure!(
        !identifier.is_empty()
            && identifier.len() <= 128
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "{label} identifier is invalid"
    );
    Ok(())
}

fn validate_date_components(components: &DateComponents) -> Result<()> {
    anyhow::ensure!(
        components.year.is_some()
            || components.month.is_some()
            || components.day.is_some()
            || components.hour.is_some()
            || components.minute.is_some()
            || components.second.is_some(),
        "calendar notification trigger must specify at least one date component"
    );
    if let Some(month) = components.month {
        anyhow::ensure!(
            (1..=12).contains(&month),
            "calendar month must be between 1 and 12"
        );
    }
    if let Some(day) = components.day {
        anyhow::ensure!(
            (1..=31).contains(&day),
            "calendar day must be between 1 and 31"
        );
    }
    if let Some(hour) = components.hour {
        anyhow::ensure!(hour <= 23, "calendar hour must be between 0 and 23");
    }
    if let Some(minute) = components.minute {
        anyhow::ensure!(minute <= 59, "calendar minute must be between 0 and 59");
    }
    if let Some(second) = components.second {
        anyhow::ensure!(second <= 59, "calendar second must be between 0 and 59");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use parking_lot::Mutex;

    use crate::{
        action::{ActionOptions, NotificationAction, NotificationCategory},
        platform::NotificationBackend,
        push::PushToken,
    };

    use super::{
        AuthorizationOptions, LocalNotification, NotificationAttachment, NotificationCenter,
        NotificationEvent, NotificationPayload, NotificationTrigger,
    };

    #[derive(Default)]
    struct MockBackend {
        delivered: Mutex<Vec<NotificationPayload>>,
        action_to_emit: Mutex<Option<String>>,
        fail_delivery: AtomicBool,
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
            if self.fail_delivery.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("delivery failed"));
            }
            self.delivered.lock().push(payload.clone());
            if let Some(action_id) = self.action_to_emit.lock().clone() {
                if let Some(on_action) = on_action {
                    on_action(action_id);
                }
            }
            Ok(())
        }

        fn register_for_push(&self) -> anyhow::Result<PushToken> {
            Ok(PushToken::new([1, 2, 3]))
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
            badge: None,
            category: None,
            user_info: Default::default(),
            trigger: NotificationTrigger::Immediate,
            attachments: Vec::new(),
        };
        center.schedule_local(notification).unwrap();

        assert_eq!(backend.delivered.lock().len(), 1);
        assert!(
            events
                .lock()
                .iter()
                .any(|event| matches!(event, NotificationEvent::Received(_)))
        );
    }

    #[test]
    fn local_notification_constructor_uses_safe_immediate_defaults() {
        let notification = LocalNotification::new("Hello", "World");
        assert_eq!(notification.title, "Hello");
        assert_eq!(notification.body, "World");
        assert_eq!(notification.trigger, NotificationTrigger::Immediate);
        assert!(notification.attachments.is_empty());
        assert!(notification.user_info.is_empty());
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
        center
            .register_category(NotificationCategory {
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
            })
            .unwrap();
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
            events.lock().iter().any(|event| {
                matches!(
                    event,
                    NotificationEvent::ActionPerformed { action_id, .. } if action_id == "open"
                )
            })
        });
        assert!(events.lock().iter().any(|event| matches!(
            event,
            NotificationEvent::Received(payload) if payload.title == "Action"
        )));
    }

    #[test]
    fn many_delayed_notifications_use_bounded_threads() {
        let center = NotificationCenter::with_backend(Arc::new(MockBackend::default()));
        for i in 0..100 {
            let notification = LocalNotification {
                title: format!("bounded-{i}"),
                body: "test".into(),
                subtitle: None,
                sound: None,
                badge: None,
                category: None,
                user_info: Default::default(),
                trigger: NotificationTrigger::TimeInterval {
                    seconds: 3600.0,
                    repeats: false,
                },
                attachments: Vec::new(),
            };
            center.schedule_local(notification).unwrap();
        }
        assert!(center.scheduler_thread_count() <= 2);
    }

    #[test]
    fn invalid_intervals_and_exhausted_ids_return_errors_without_panicking() {
        let center = NotificationCenter::with_backend(Arc::new(MockBackend::default()));
        for seconds in [f64::NAN, f64::INFINITY, -1.0, f64::MAX] {
            let notification = test_notification(NotificationTrigger::TimeInterval {
                seconds,
                repeats: false,
            });
            assert!(center.schedule_local(notification).is_err());
        }

        center
            .inner
            .next_notification_id
            .store(u64::MAX, Ordering::Relaxed);
        assert!(
            center
                .schedule_local(test_notification(NotificationTrigger::Immediate))
                .is_err()
        );
    }

    #[test]
    fn cancellation_preserves_payload_and_removes_queued_entry() {
        let center = NotificationCenter::with_backend(Arc::new(MockBackend::default()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let _subscription = center.on_received(move |event| observed.lock().push(event));
        let id = center
            .schedule_local(test_notification(NotificationTrigger::TimeInterval {
                seconds: 3600.0,
                repeats: false,
            }))
            .unwrap();

        center.cancel(&id);

        assert!(center.inner.scheduler_queue.0.lock().is_empty());
        assert!(events.lock().iter().any(|event| matches!(
            event,
            NotificationEvent::Dismissed(payload) if payload.title == "Test"
        )));
    }

    #[test]
    fn cancelling_all_emits_dismissed_events_and_clears_the_queue() {
        let center = NotificationCenter::with_backend(Arc::new(MockBackend::default()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let _subscription = center.on_received(move |event| observed.lock().push(event));
        for _ in 0..2 {
            center
                .schedule_local(test_notification(NotificationTrigger::TimeInterval {
                    seconds: 3600.0,
                    repeats: false,
                }))
                .unwrap();
        }

        center.cancel_all();

        assert!(center.inner.scheduler_queue.0.lock().is_empty());
        assert_eq!(
            events
                .lock()
                .iter()
                .filter(|event| matches!(event, NotificationEvent::Dismissed(_)))
                .count(),
            2
        );
    }

    #[test]
    fn failed_delivery_is_removed_from_scheduled_state() {
        let backend = Arc::new(MockBackend::default());
        backend.fail_delivery.store(true, Ordering::Relaxed);
        let center = NotificationCenter::with_backend(backend);
        let id = center
            .schedule_local(test_notification(NotificationTrigger::TimeInterval {
                seconds: 0.01,
                repeats: false,
            }))
            .unwrap();

        wait_until(Duration::from_secs(5), || {
            !center.inner.scheduled.lock().contains_key(&id)
        });
    }

    #[test]
    fn listener_panics_are_isolated_and_categories_are_validated() {
        let center = NotificationCenter::with_backend(Arc::new(MockBackend::default()));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        let _panicking = center.on_received(|_| panic!("listener failure"));
        let _healthy = center.on_received(move |_| {
            calls_for_listener.fetch_add(1, Ordering::Relaxed);
        });

        center
            .schedule_local(test_notification(NotificationTrigger::Immediate))
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        assert!(
            center
                .register_category(NotificationCategory {
                    identifier: "duplicate".into(),
                    actions: vec![
                        NotificationAction {
                            identifier: "open".into(),
                            title: "Open".into(),
                            options: ActionOptions::default(),
                            text_input_placeholder: None,
                        },
                        NotificationAction {
                            identifier: "open".into(),
                            title: "Again".into(),
                            options: ActionOptions::default(),
                            text_input_placeholder: None,
                        },
                    ],
                })
                .is_err()
        );
    }

    #[test]
    fn unsupported_payloads_fail_instead_of_being_silently_ignored() {
        let center = NotificationCenter::with_backend(Arc::new(MockBackend::default()));
        let mut notification = test_notification(NotificationTrigger::Immediate);
        notification.attachments.push(NotificationAttachment {
            path: "preview.png".into(),
            mime_type: Some("image/png".into()),
        });

        assert!(center.schedule_local(notification).is_err());
        assert!(center.set_badge_count(1).is_err());
        assert!(
            center
                .register_category(NotificationCategory {
                    identifier: "reply".into(),
                    actions: vec![NotificationAction {
                        identifier: "reply".into(),
                        title: "Reply".into(),
                        options: ActionOptions::default(),
                        text_input_placeholder: Some("Message".into()),
                    }],
                })
                .is_err()
        );
    }

    fn test_notification(trigger: NotificationTrigger) -> LocalNotification {
        LocalNotification {
            title: "Test".into(),
            body: "Body".into(),
            subtitle: None,
            sound: None,
            badge: None,
            category: None,
            user_info: Default::default(),
            trigger,
            attachments: Vec::new(),
        }
    }

    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if predicate() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if predicate() {
            return;
        }
        panic!("condition was not satisfied before timeout");
    }
}
