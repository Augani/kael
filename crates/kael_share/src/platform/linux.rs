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
    let attachments = sheet.attachment_paths()?;

    if !sheet.is_excluded(ShareType::Mail) && launch_email(sheet, &attachments)? {
        return Ok(ShareResult::Completed {
            activity_type: ShareType::Mail.activity_name().to_string(),
        });
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

    if launch_open(sheet, &attachments)? {
        return Ok(ShareResult::Completed {
            activity_type: "open".to_string(),
        });
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
        mail: true,
        messages: false,
        airdrop: false,
        clipboard: true,
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
    if let Some(url) = sheet.all_urls().first() {
        command.arg(url);
    }
    spawn_command(command)
}

fn launch_open(sheet: &ShareSheet, attachments: &[std::path::PathBuf]) -> Result<bool> {
    for url in sheet.all_urls() {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        if spawn_command(command)? {
            return Ok(true);
        }
    }

    if let Some(first_path) = attachments.first() {
        let mut command = Command::new("xdg-open");
        command.arg(first_path);
        return spawn_command(command);
    }

    Ok(false)
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
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                return Ok(true);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Ok(false)
}

fn spawn_command(mut command: Command) -> Result<bool> {
    match command.spawn() {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
