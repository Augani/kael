use anyhow::Result;

use crate::{ReceiverCallback, ShareFileType, ShareResult, ShareSheet};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

/// Summary of the destinations the current platform backend can launch today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformShareSupport {
    /// Whether the backend can hand off to a mail composer.
    pub mail: bool,
    /// Whether the backend can hand off to a messages destination.
    pub messages: bool,
    /// Whether the backend can hand off to AirDrop or a nearby equivalent.
    pub airdrop: bool,
    /// Whether the backend can copy payloads to the clipboard.
    pub clipboard: bool,
    /// Whether the backend can hand off to a social-posting target.
    pub social: bool,
    /// Whether the backend can hand off to the system print flow.
    pub print: bool,
    /// Whether the application can register as a share target.
    pub receiver_registration: bool,
}

impl PlatformShareSupport {
    /// Number of destination families supported by the current backend.
    pub fn supported_count(&self) -> usize {
        [
            self.mail,
            self.messages,
            self.airdrop,
            self.clipboard,
            self.social,
            self.print,
            self.receiver_registration,
        ]
        .into_iter()
        .filter(|supported| *supported)
        .count()
    }

    /// Whether no share destination family is currently available.
    pub fn is_empty(&self) -> bool {
        self.supported_count() == 0
    }

    /// Human-readable, content-safe support summary for logs and agents.
    pub fn to_text(&self) -> String {
        format!(
            "share support: {} supported, mail {}, messages {}, airdrop {}, clipboard {}, social {}, print {}, receiver {}",
            self.supported_count(),
            self.mail,
            self.messages,
            self.airdrop,
            self.clipboard,
            self.social,
            self.print,
            self.receiver_registration
        )
    }
}

pub(crate) struct PlatformShareReceiver;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(target_os = "windows")]
use windows as imp;

pub(crate) async fn show(sheet: &ShareSheet) -> Result<ShareResult> {
    imp::show(sheet).await
}

pub(crate) fn register_receiver(
    file_types: &[ShareFileType],
    callback: ReceiverCallback,
) -> Result<PlatformShareReceiver> {
    imp::register_receiver(file_types, callback)
}

pub(crate) fn support() -> PlatformShareSupport {
    imp::support()
}
