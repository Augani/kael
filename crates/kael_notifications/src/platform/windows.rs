use std::sync::{Arc, OnceLock};

use anyhow::{Result, anyhow};
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager},
    core::{HSTRING, Interface as _},
};

use crate::{
    action::NotificationAction,
    local::{LocalNotification, NotificationPayload},
};

use super::{NotificationBackend, PlatformNotificationSupport};

pub(crate) struct PlatformBackend;

pub(crate) const SUPPORT: PlatformNotificationSupport = PlatformNotificationSupport {
    delivery_backend: "windows-runtime-toast",
    action_backend: "windows-runtime-toast-activation",
    push_backend: "not-implemented",
};

impl NotificationBackend for PlatformBackend {
    fn deliver(
        &self,
        payload: &NotificationPayload,
        notification: &LocalNotification,
        actions: &[NotificationAction],
        on_action: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
    ) -> Result<()> {
        let document = XmlDocument::new()
            .map_err(|error| anyhow!("failed to create toast document: {error}"))?;
        document
            .LoadXml(&HSTRING::from(toast_xml(payload, notification, actions)))
            .map_err(|error| anyhow!("failed to load toast XML: {error}"))?;
        let toast = ToastNotification::CreateToastNotification(&document)
            .map_err(|error| anyhow!("failed to create toast notification: {error}"))?;

        if let Some(on_action) = on_action.filter(|_| !actions.is_empty()) {
            toast
                .Activated(&TypedEventHandler::<
                    ToastNotification,
                    windows::core::IInspectable,
                >::new(move |_sender, args| {
                    if let Some(args) = args.as_ref() {
                        if let Ok(args) = args.cast::<ToastActivatedEventArgs>() {
                            if let Ok(arguments) = args.Arguments() {
                                let action_id = arguments.to_string();
                                if !action_id.is_empty() {
                                    on_action(action_id);
                                }
                            }
                        }
                    }
                    Ok(())
                }))
                .map_err(|error| anyhow!("failed to attach toast action handler: {error}"))?;
        }

        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_user_model_id()))
            .and_then(|notifier| notifier.Show(&toast))
            .map_err(|error| anyhow!("failed to show notification: {error}"))?;
        Ok(())
    }
}

// Allows unpackaged development builds to show notifications. Packaged apps should
// register their own AppUserModelID; this fallback matches Windows' documented shell ID.
const POWERSHELL_APP_USER_MODEL_ID: &str = concat!(
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}",
    "\\WindowsPowerShell\\v1.0\\powershell.exe"
);

const MAX_APP_USER_MODEL_ID_CHARS: usize = 128;
static APP_USER_MODEL_ID: OnceLock<String> = OnceLock::new();

pub(crate) fn set_app_user_model_id(id: String) -> Result<()> {
    let id = id.trim();
    anyhow::ensure!(!id.is_empty(), "Windows AppUserModelID must not be empty");
    anyhow::ensure!(
        !id.contains('\0'),
        "Windows AppUserModelID must not contain a NUL character"
    );
    anyhow::ensure!(
        id.chars().count() <= MAX_APP_USER_MODEL_ID_CHARS,
        "Windows AppUserModelID exceeds {MAX_APP_USER_MODEL_ID_CHARS} characters"
    );
    APP_USER_MODEL_ID
        .set(id.to_owned())
        .map_err(|_| anyhow!("Windows AppUserModelID was already configured"))
}

fn app_user_model_id() -> &'static str {
    APP_USER_MODEL_ID
        .get()
        .map(String::as_str)
        .unwrap_or(POWERSHELL_APP_USER_MODEL_ID)
}

fn toast_xml(
    payload: &NotificationPayload,
    notification: &LocalNotification,
    actions: &[NotificationAction],
) -> String {
    let mut xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text>",
        escape_xml(&payload.title)
    );
    if let Some(subtitle) = notification
        .subtitle
        .as_deref()
        .filter(|subtitle| !subtitle.is_empty())
    {
        xml.push_str(&format!("<text>{}</text>", escape_xml(subtitle)));
    }
    xml.push_str(&format!(
        "<text>{}</text></binding></visual>",
        escape_xml(&notification.body)
    ));
    if !actions.is_empty() {
        xml.push_str("<actions>");
        for action in actions {
            xml.push_str(&format!(
                "<action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/>",
                escape_xml(&action.title),
                escape_xml(&action.identifier)
            ));
        }
        xml.push_str("</actions>");
    }
    xml.push_str("</toast>");
    xml
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
