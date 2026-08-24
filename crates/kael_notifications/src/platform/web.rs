use std::{cell::RefCell, collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

use anyhow::Result;
use js_sys::{Reflect, global};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, Notification, NotificationOptions};

use crate::{
    NotificationError, NotificationId, NotificationOperationResult, NotificationPermissionStatus,
    action::NotificationAction,
    local::{AuthorizationOptions, LocalNotification, NotificationPayload, NotificationSound},
};

use super::{NotificationBackend, PlatformNotificationSupport};

const MAX_ACTIVE_NOTIFICATIONS: usize = 256;

type PermissionFuture<'a> =
    Pin<Box<dyn Future<Output = NotificationOperationResult<NotificationPermissionStatus>> + 'a>>;

trait BrowserPermissionDriver {
    fn status(&self) -> NotificationPermissionStatus;
    fn user_activation_active(&self) -> Option<bool>;
    fn prompt<'a>(&'a self) -> PermissionFuture<'a>;
}

struct NotificationPermissionDriver;

impl BrowserPermissionDriver for NotificationPermissionDriver {
    fn status(&self) -> NotificationPermissionStatus {
        permission_status()
    }

    fn user_activation_active(&self) -> Option<bool> {
        let navigator = Reflect::get(&global(), &JsValue::from_str("navigator")).ok()?;
        let activation = Reflect::get(&navigator, &JsValue::from_str("userActivation")).ok()?;
        Reflect::get(&activation, &JsValue::from_str("isActive"))
            .ok()?
            .as_bool()
    }

    fn prompt<'a>(&'a self) -> PermissionFuture<'a> {
        Box::pin(async move {
            let promise = Notification::request_permission()
                .map_err(|_| NotificationError::PermissionPromptRequired)?;
            let value = JsFuture::from(promise)
                .await
                .map_err(|_| NotificationError::PermissionDenied)?;
            match value.as_string().as_deref() {
                Some("granted") => Ok(NotificationPermissionStatus::Granted),
                Some("denied") | Some("default") => Ok(NotificationPermissionStatus::Denied),
                _ => Err(NotificationError::Platform(
                    "the browser returned an unknown notification permission".into(),
                )),
            }
        })
    }
}

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "browser-notification-api",
    action_backend: "notification-body-click-only",
    push_backend: "service-worker-integration-required",
    asynchronous_permission: true,
    immediate_delivery: true,
    interval_triggers: false,
    system_triggers: false,
    actions: false,
    cancellation: true,
};

struct ActiveNotification {
    notification: Notification,
    _on_click: Option<Closure<dyn FnMut(Event)>>,
}

thread_local! {
    static ACTIVE: RefCell<BTreeMap<NotificationId, ActiveNotification>> =
        const { RefCell::new(BTreeMap::new()) };
}

impl NotificationBackend for PlatformBackend {
    fn request_authorization(&self, _options: &AuthorizationOptions) -> Result<bool> {
        match permission_status() {
            NotificationPermissionStatus::Granted
            | NotificationPermissionStatus::PlatformManaged => Ok(true),
            NotificationPermissionStatus::Denied => Ok(false),
            NotificationPermissionStatus::Prompt => Err(anyhow::Error::new(
                NotificationError::PermissionPromptRequired,
            )),
            NotificationPermissionStatus::Unavailable => {
                Err(anyhow::Error::new(NotificationError::Unavailable))
            }
        }
    }

    fn request_authorization_async<'a>(
        &'a self,
        _options: &'a AuthorizationOptions,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + 'a>> {
        Box::pin(async move {
            resolve_permission(&NotificationPermissionDriver)
                .await
                .map(|status| status == NotificationPermissionStatus::Granted)
                .map_err(anyhow::Error::new)
        })
    }

    fn deliver(
        &self,
        payload: &NotificationPayload,
        notification: &LocalNotification,
        actions: &[NotificationAction],
        on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
    ) -> Result<()> {
        match permission_status() {
            NotificationPermissionStatus::Granted
            | NotificationPermissionStatus::PlatformManaged => {}
            NotificationPermissionStatus::Prompt => {
                return Err(anyhow::Error::new(
                    NotificationError::PermissionPromptRequired,
                ));
            }
            NotificationPermissionStatus::Denied => {
                return Err(anyhow::Error::new(NotificationError::PermissionDenied));
            }
            NotificationPermissionStatus::Unavailable => {
                return Err(anyhow::Error::new(NotificationError::Unavailable));
            }
        }
        if !actions.is_empty() {
            return Err(anyhow::Error::new(NotificationError::UnsupportedFeature(
                "action buttons require a product service worker",
            )));
        }
        if notification.badge.is_some() {
            return Err(anyhow::Error::new(NotificationError::UnsupportedFeature(
                "application badge count",
            )));
        }
        if matches!(notification.sound, Some(NotificationSound::Named(_))) {
            return Err(anyhow::Error::new(NotificationError::UnsupportedFeature(
                "named notification sounds",
            )));
        }

        let body = match notification
            .subtitle
            .as_deref()
            .filter(|subtitle| !subtitle.is_empty())
        {
            Some(subtitle) if notification.body.is_empty() => subtitle.to_string(),
            Some(subtitle) => format!("{subtitle}\n{}", notification.body),
            None => notification.body.clone(),
        };
        let options = NotificationOptions::new();
        options.set_body(&body);
        if matches!(notification.sound, Some(NotificationSound::Silent)) {
            options.set_silent(Some(true));
        }

        let browser_notification = Notification::new_with_options(&payload.title, &options)
            .map_err(|_| anyhow::Error::new(NotificationError::PermissionDenied))?;
        let on_click = on_action.map(|on_action| {
            let callback = Closure::<dyn FnMut(Event)>::new(move |_event| {
                on_action(crate::DEFAULT_NOTIFICATION_ACTION_ID.to_string());
                if let Some(window) = web_sys::window() {
                    let _ = window.focus();
                }
            });
            browser_notification.set_onclick(Some(callback.as_ref().unchecked_ref()));
            callback
        });

        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if active.len() >= MAX_ACTIVE_NOTIFICATIONS
                && let Some(oldest) = active.keys().next().copied()
                && let Some(old) = active.remove(&oldest)
            {
                old.notification.set_onclick(None);
                old.notification.close();
            }
            active.insert(
                payload.id,
                ActiveNotification {
                    notification: browser_notification,
                    _on_click: on_click,
                },
            );
        });
        Ok(())
    }

    fn cancel(&self, id: NotificationId) {
        ACTIVE.with(|active| {
            if let Some(entry) = active.borrow_mut().remove(&id) {
                entry.notification.set_onclick(None);
                entry.notification.close();
            }
        });
    }

    fn cancel_all(&self) {
        ACTIVE.with(|active| {
            for (_, entry) in std::mem::take(&mut *active.borrow_mut()) {
                entry.notification.set_onclick(None);
                entry.notification.close();
            }
        });
    }
}

pub(crate) fn permission_status() -> NotificationPermissionStatus {
    if !api_available() {
        return NotificationPermissionStatus::Unavailable;
    }
    Reflect::get(&global(), &JsValue::from_str("Notification"))
        .ok()
        .and_then(|constructor| {
            Reflect::get(&constructor, &JsValue::from_str("permission"))
                .ok()
                .and_then(|permission| permission.as_string())
        })
        .map_or(
            NotificationPermissionStatus::Unavailable,
            |permission| match permission.as_str() {
                "granted" => NotificationPermissionStatus::Granted,
                "denied" => NotificationPermissionStatus::Denied,
                "default" => NotificationPermissionStatus::Prompt,
                _ => NotificationPermissionStatus::Unavailable,
            },
        )
}

fn api_available() -> bool {
    Reflect::get(&global(), &JsValue::from_str("Notification"))
        .is_ok_and(|value| value.is_function())
}

async fn resolve_permission(
    driver: &dyn BrowserPermissionDriver,
) -> NotificationOperationResult<NotificationPermissionStatus> {
    match driver.status() {
        NotificationPermissionStatus::Granted | NotificationPermissionStatus::PlatformManaged => {
            Ok(NotificationPermissionStatus::Granted)
        }
        NotificationPermissionStatus::Denied => Ok(NotificationPermissionStatus::Denied),
        NotificationPermissionStatus::Unavailable => Err(NotificationError::Unavailable),
        NotificationPermissionStatus::Prompt => {
            if driver.user_activation_active() == Some(false) {
                return Err(NotificationError::UserActivationRequired);
            }
            driver.prompt().await
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use crate::{NotificationError, NotificationPermissionStatus};

    use super::{BrowserPermissionDriver, PermissionFuture, resolve_permission};

    wasm_bindgen_test_configure!(run_in_browser);

    struct MockPermissionDriver {
        status: NotificationPermissionStatus,
        active: Option<bool>,
        prompted: Rc<Cell<usize>>,
        prompt_result: Result<NotificationPermissionStatus, NotificationError>,
    }

    impl BrowserPermissionDriver for MockPermissionDriver {
        fn status(&self) -> NotificationPermissionStatus {
            self.status
        }

        fn user_activation_active(&self) -> Option<bool> {
            self.active
        }

        fn prompt<'a>(&'a self) -> PermissionFuture<'a> {
            self.prompted.set(self.prompted.get() + 1);
            let result = self.prompt_result.clone();
            Box::pin(async move { result })
        }
    }

    fn driver(
        status: NotificationPermissionStatus,
        prompt_result: Result<NotificationPermissionStatus, NotificationError>,
    ) -> MockPermissionDriver {
        MockPermissionDriver {
            status,
            active: Some(true),
            prompted: Rc::new(Cell::new(0)),
            prompt_result,
        }
    }

    #[wasm_bindgen_test(async)]
    async fn granted_and_denied_policy_never_opens_os_ui() {
        let granted = driver(
            NotificationPermissionStatus::Granted,
            Ok(NotificationPermissionStatus::Denied),
        );
        assert_eq!(
            resolve_permission(&granted).await.unwrap(),
            NotificationPermissionStatus::Granted
        );
        assert_eq!(granted.prompted.get(), 0);

        let denied = driver(
            NotificationPermissionStatus::Denied,
            Ok(NotificationPermissionStatus::Granted),
        );
        assert_eq!(
            resolve_permission(&denied).await.unwrap(),
            NotificationPermissionStatus::Denied
        );
        assert_eq!(denied.prompted.get(), 0);
    }

    #[wasm_bindgen_test(async)]
    async fn prompt_and_unavailable_policy_are_injected_and_typed() {
        let prompt = driver(
            NotificationPermissionStatus::Prompt,
            Ok(NotificationPermissionStatus::Denied),
        );
        assert_eq!(
            resolve_permission(&prompt).await.unwrap(),
            NotificationPermissionStatus::Denied
        );
        assert_eq!(prompt.prompted.get(), 1);

        let unavailable = driver(
            NotificationPermissionStatus::Unavailable,
            Ok(NotificationPermissionStatus::Granted),
        );
        assert_eq!(
            resolve_permission(&unavailable).await.unwrap_err(),
            NotificationError::Unavailable
        );
        assert_eq!(unavailable.prompted.get(), 0);

        let mut inactive = driver(
            NotificationPermissionStatus::Prompt,
            Ok(NotificationPermissionStatus::Granted),
        );
        inactive.active = Some(false);
        assert_eq!(
            resolve_permission(&inactive).await.unwrap_err(),
            NotificationError::UserActivationRequired
        );
        assert_eq!(inactive.prompted.get(), 0);
    }
}
