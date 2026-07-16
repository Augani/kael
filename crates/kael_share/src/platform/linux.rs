use anyhow::{Result, anyhow};
use std::{
    io::Write,
    process::{Command, Stdio},
};

use crate::{
    ReceiverCallback, ShareFileType, ShareResult, ShareSheet, ShareType,
    platform::PlatformShareReceiver,
};

pub(crate) async fn show(sheet: &ShareSheet) -> Result<ShareResult> {
    let sheet = sheet.clone();
    smol::unblock(move || show_blocking(&sheet)).await
}

fn show_blocking(sheet: &ShareSheet) -> Result<ShareResult> {
    if !sheet.is_excluded(ShareType::Mail) {
        let attachments = sheet.attachment_paths()?;
        if launch_email(sheet, &attachments)? {
            return Ok(ShareResult::Completed {
                activity_type: ShareType::Mail.activity_name().to_string(),
            });
        }
    }

    if !sheet.is_excluded(ShareType::Clipboard) {
        if let Some(body) = sheet.body_text() {
            if copy_to_clipboard(&body)? {
                return Ok(ShareResult::Completed {
                    activity_type: ShareType::Clipboard.activity_name().to_string(),
                });
            }
        }
    }

    Ok(ShareResult::Cancelled)
}

pub(crate) fn register_receiver(
    _file_types: &[ShareFileType],
    _callback: ReceiverCallback,
) -> Result<PlatformShareReceiver> {
    Err(anyhow!(
        "share receiver registration is not implemented yet on Linux"
    ))
}

pub(crate) fn support() -> crate::PlatformShareSupport {
    crate::PlatformShareSupport {
        mail: executable_in_path("xdg-email"),
        messages: false,
        airdrop: false,
        clipboard: ["wl-copy", "xclip", "xsel"]
            .into_iter()
            .any(executable_in_path),
        social: false,
        print: false,
        receiver_registration: false,
    }
}

fn launch_email(sheet: &ShareSheet, attachments: &[std::path::PathBuf]) -> Result<bool> {
    let mut command = Command::new("xdg-email");
    if let Some(subject) = sheet.first_subject() {
        command.arg("--subject").arg(subject);
    }
    if let Some(body) = sheet.body_text() {
        command.arg("--body").arg(body);
    }
    for attachment in attachments {
        command.arg("--attach").arg(attachment);
    }
    spawn_command(command)
}

fn copy_to_clipboard(text: &str) -> Result<bool> {
    for (program, args) in [
        ("wl-copy", Vec::<&str>::new()),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        let mut command = Command::new(program);
        command.args(&args).stdin(Stdio::piped());
        match command.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes())?;
                }
                return child
                    .wait()
                    .map(|status| status.success())
                    .map_err(Into::into);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Ok(false)
}

fn spawn_command(mut command: Command) -> Result<bool> {
    match command.status() {
        Ok(status) => Ok(status.success()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn executable_in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| {
            let path = directory.join(program);
            path.is_file()
        })
    })
}
