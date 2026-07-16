use crate::{DialogKind, DialogOptions};
use futures::channel::oneshot;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSModalResponse};
use objc2_foundation::NSString;

const NS_ALERT_FIRST_BUTTON_RETURN: NSModalResponse = 1000;

fn dialog_response_index(response: NSModalResponse, button_count: usize) -> usize {
    let button_count = button_count.max(1);
    response
        .checked_sub(NS_ALERT_FIRST_BUTTON_RETURN)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < button_count)
        .unwrap_or_default()
}

pub fn show_dialog(options: DialogOptions) -> oneshot::Receiver<usize> {
    let (tx, rx) = oneshot::channel();
    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("show_dialog called off the macOS main thread; selecting the first answer");
        tx.send(0).ok();
        return rx;
    };
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
    let index = dialog_response_index(response, options.buttons.len());
    tx.send(index).ok();

    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_responses_are_bounded_to_available_buttons() {
        assert_eq!(dialog_response_index(1000, 3), 0);
        assert_eq!(dialog_response_index(1002, 3), 2);
        assert_eq!(dialog_response_index(999, 3), 0);
        assert_eq!(dialog_response_index(1003, 3), 0);
        assert_eq!(dialog_response_index(isize::MAX, 3), 0);
        assert_eq!(dialog_response_index(1000, 0), 0);
    }
}
