use crate::{DialogKind, DialogOptions};
use futures::channel::oneshot;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSModalResponse};
use objc2_foundation::NSString;

const NS_ALERT_FIRST_BUTTON_RETURN: NSModalResponse = 1000;

pub fn show_dialog(options: DialogOptions) -> oneshot::Receiver<usize> {
    let (tx, rx) = oneshot::channel();
    let mtm = MainThreadMarker::new().expect("show_dialog must be called on the main thread");
    let alert = NSAlert::new(mtm);

    let style = match options.kind {
        DialogKind::Info => NSAlertStyle::Informational,
        DialogKind::Warning => NSAlertStyle::Warning,
        DialogKind::Error => NSAlertStyle::Critical,
    };
    alert.setAlertStyle(style);

    let title = NSString::from_str(options.title.as_ref());
    alert.setMessageText(&title);

    let informative = if let Some(detail) = &options.detail {
        format!("{}\n\n{}", options.message.as_ref(), detail.as_ref())
    } else {
        options.message.as_ref().to_string()
    };
    let message = NSString::from_str(&informative);
    alert.setInformativeText(&message);

    for button_label in &options.buttons {
        let label = NSString::from_str(button_label.as_ref());
        alert.addButtonWithTitle(&label);
    }
    if options.buttons.is_empty() {
        let ok_label = NSString::from_str("OK");
        alert.addButtonWithTitle(&ok_label);
    }

    let response = alert.runModal();
    let index = (response - NS_ALERT_FIRST_BUTTON_RETURN) as usize;
    tx.send(index).ok();

    rx
}
